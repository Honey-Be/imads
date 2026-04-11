# アーキテクチャ概要

## Engine と PolicyBundle

エンジン (`Engine<P: PolicyBundle>`) は、プラグイン可能なポリシーサーフェスを通じて最適化を統括します。各ポリシースロットは `PolicyBundle` の関連型として定義されています。

| Policy | 役割 | カスタマイズ可能？ |
|--------|------|:---:|
| `SchedulerPolicy` | バッチディスパッチの順序制御 | Yes |
| `SearchPolicy` | 候補の生成とスコアリング | Yes |
| `LadderPolicy` | (tau, S) フィデリティラダーの構築 | Yes |
| `DidsPolicy` | 動的実行不能度戦略 | Yes |
| `MarginPolicy` | 早期実行不能/目的関数閾値 | Yes |
| `CalibratorPolicy` | デルタコントローラーと K 学習 | Yes |
| `AuditPolicy` | ハッシュベースの監査選択 | Yes |
| `AcceptancePolicy` | フィルター＋プログレッシブバリア受理 | Yes |
| `EvalCacheBackend` | 推定値キャッシュ | Yes |
| `DecisionCacheBackend` | 判定キャッシュ | Yes |
| `Executor` | ワークバッチの実行 | Yes |

**Sealed（カスタマイズ不可）：**
- Poll/メッシュ更新 (`DefaultPoll`) — 収束に不可欠

> **注意:** `AcceptancePolicy` は以前 `AcceptanceEngine` として封印されていました。現在は
> パブリックトレイトになっています。`DefaultAcceptance` は `AcceptancePolicy` を実装しており、
> デフォルトのままです。ユーザーはカスタムの受理ポリシーを実装できます（例: 多目的最適化向けの
> パレートベース）。

## AdaptiveExecutor

`DefaultBundle` は `AdaptiveExecutor` を使用し、以下のように自動選択します。

- **workers = 1** → `InlineExecutor`（逐次実行、オーバーヘッドゼロ）
- **workers > 1** → `WorkerPoolExecutor`（バッチバリア付きスレッドプール）

スレッドサポートのない WASM ターゲットでは、`InlineExecutor` のみが利用可能です。
`wasm32-wasip1-threads` および `wasm32-wasip3` では、プール版が有効になります。

### Evaluator トレイト

`Evaluator` トレイトはブラックボックスインターフェースを定義します:

| メソッド / 型 | 必須 | 説明 |
|---------------|----------|-------------|
| `type Objectives: ObjectiveValues` | はい | 目的関数値の関連型（f64, [f64;N], または Vec<f64>） |
| `mc_sample(x, phi, env, k)` | はい | 目的関数＋制約の決定論的 MC サンプル |
| `cheap_constraints(x, env)` | いいえ | 高速棄却ゲート（デフォルト: すべて受理） |
| `solver_bias(x, tau, env)` | いいえ | Tau 依存バイアス項（デフォルト: ゼロ） |
| `num_constraints()` | はい | 制約値の数 |
| `num_objectives()` | はい | 目的関数値の数（単目的の場合は 1） |
| `search_dim()` | いいえ | 探索空間の次元; `Some(d)` の場合、`EngineConfig.search_dim` をオーバーライド |

エンジンは次元を以下の優先順位で解決します: `config.search_dim` > `evaluator.search_dim()` > 現在の解の長さ > フォールバック 1。

### ObjectiveValues と多目的サポート

`ObjectiveValues` トレイトは、単目的および多目的の evaluator を抽象化します。`f64`（単目的）、
`[f64; N]`（固定数）、および `Vec<f64>`（動的数）に対して実装されています。

- `Estimates.f_hat` と `f_se` は `Vec<f64>`（目的関数ごとに 1 エントリ）です。
- `Estimates.num_objectives` はその数を報告します。
- `JobResult::Truth.f` は `Vec<f64>` です。
- `EngineOutput.f_best` は `Option<Vec<f64>>` です。

マーカーサブトレイト `SingleObjectiveEvaluator` は、`Objectives = f64` を持つ任意の evaluator に対してブランケット impl を持ち、後方互換性を維持します。

### EvaluatorErased

`EvaluatorErased` は、エンジン内部で使用される型消去トレイトオブジェクトラッパーです。
ジェネリック感染を回避します。エンジンコアは具体的な evaluator 型でパラメータ化されるのではなく、
`&dyn EvaluatorErased` 上で動作します。ユーザーコードはこのトレイトと直接やり取りする必要はありません。

## StratifiedSearch

`StratifiedSearch` は `DefaultSearch` のドロップイン置換です（`PolicyBundle::Search` 経由）。
`imads-core/src/policies/stratified_search.rs` で定義されており、3 つの候補生成モードを
組み合わせます:

1. **座標ステップ** — 最大 `min(dim, 6)` 個の座標方向に対して、メッシュ整合された摂動でポーリングします。
2. **方向探索** — 最近の成功ステップから導出された改善ベクトルに沿って外挿します。
3. **Halton 準ランダムグローバル探索** — グローバルカバレッジのために、探索空間全体にわたって低食い違い点を生成します。

これらのモード間の配分比率は、問題の次元数に基づいて動的に調整されます:

| 次元数 | 座標 | 方向 | Halton |
|:--------------:|:----------:|:-----------:|:------:|
| dim <= 8       | 60%        | 20%         | 20%    |
| 8 < dim < 32   | 45%        | 25%         | 30%    |
| dim >= 32      | 30%        | 30%         | 40%    |

高次元の問題では、座標ステップの効率が低下するため、グローバル探索の予算がより多く割り当てられます。

## AnisotropicMeshGeometry

`AnisotropicMeshGeometry` は、**次元ごとのメッシュステップサイズ**を有効にします。すべての次元で
共有される単一のスカラーメッシュステップの代わりに、各次元が独自の `base_step` と `mesh_mul` を
持ちます:

- `base_steps: Vec<f64>` — 次元ごとの初期ステップサイズ。
- `mesh_muls: Vec<f64>` — 次元ごとのメッシュサイズ乗数。

`EngineConfig` には `mesh_base_steps: Option<Vec<f64>>` が含まれるようになりました。`Some(steps)` の場合、
エンジンはデフォルトの等方的ジオメトリの代わりに `AnisotropicMeshGeometry` を構築します。

`SearchContext::mesh_step` は `mesh_steps: Vec<f64>`（次元ごとのステップのベクトル）に
置き換えられました。後方互換性のための `mesh_step()` アクセサは、等方的メッシュを前提とする
コード向けに最初の要素を返します。

`env_rev_with_steps()` 関数はキャッシュキーハッシュに `base_steps` を含めることで、
異なる異方的設定が評価キャッシュ内で衝突しないことを保証します。

## 三段階判定フロー

1. **ステージ A（低コスト）** — `Evaluator::cheap_constraints()`。ブラックボックス評価なしで棄却します。
2. **PARTIAL** — 中間の (tau, S) フィデリティ。早期実行不能判定または停止をトリガーする場合があります。
3. **TRUTH** — 最高フィデリティでの最終評価。TRUTH のみがフィルターに受理されます。

## フィデリティラダー

2 軸ラダーは `tau_levels`（許容誤差、緩→厳）と `smc_levels`（モンテカルロサンプル数、低→高）によって定義されます。`LadderPolicy` はこれらを `Phi = (Tau, Smc)` ステップの順序付きシーケンスに統合します。MC プレフィックス再利用により、ステップ i のサンプルはステップ i+1 で再利用されます。

## 決定性の契約

すべてのポリシー判定は (inputs, env_rev, policy_rev) の純粋関数です。判定パスにおいて、実時間、スレッド競合、OS 乱数は一切使用しません。これにより以下が保証されます。
- マシン間で再現可能な実行
- 1 ワーカーと N ワーカーで同一の結果
- 決定論的キーによるキャッシュの正確性

## キャリブレーターフィードバックループ

キャリブレーターは以下を追跡します。
- 制約ごと・フィデリティレベルごとの偽実行不能率
- ペア監査サンプルから学習された K（バイアス上限）
- 目標偽陽性率に向けて EWMA で調整されるデルタ閾値

更新はバッチ境界において決定論的な順序で行われます。
