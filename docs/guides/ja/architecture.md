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
| `EvalCacheBackend` | 推定値キャッシュ | Yes |
| `DecisionCacheBackend` | 判定キャッシュ | Yes |
| `Executor` | ワークバッチの実行 | Yes |

**Sealed（カスタマイズ不可）：**
- Poll/メッシュ更新 (`DefaultPoll`) — 収束に不可欠
- 受理ロジック (`DefaultAcceptance`) — フィルター＋プログレッシブバリア

## AdaptiveExecutor

`DefaultBundle` は `AdaptiveExecutor` を使用し、以下のように自動選択します。

- **workers = 1** → `InlineExecutor`（逐次実行、オーバーヘッドゼロ）
- **workers > 1** → `WorkerPoolExecutor`（バッチバリア付きスレッドプール）

スレッドサポートのない WASM ターゲットでは、`InlineExecutor` のみが利用可能です。
`wasm32-wasip1-threads` および `wasm32-wasip3` では、プール版が有効になります。

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
