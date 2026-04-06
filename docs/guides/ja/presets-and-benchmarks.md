# プリセットと小規模ベンチマークワークフロー

このプロジェクトでは現在、`src/presets.rs` に5つのプリセットを同梱しています。

- `legacy_baseline`: アップグレード5以前の動作を近似する比較専用プリセットです
- `balanced`: **推奨デフォルト**。より少ない partial-step 予算でスループット相当の品質に到達します
- `conservative`: false infeasible のリスクが最も重要な場合に、より安全かつ慎重に動作します
- `throughput`: 品質最優先のプリセット。より多くの partial-step 予算を使い、適応を高速化します

## 推奨される使い方

以下の意図に応じてプリセットを使い分けてください。

- **デフォルト / 一般ユーザー**: `balanced`
- **安全性 / デバッグ / ノイズの多い評価器**: `conservative`
- **品質最優先のスイープ**: `throughput`
- **前後比較のみ**: `legacy_baseline`

最近のレポート (`imads/reports/preset_report.csv`) では以下の結果が示されました。

- `balanced` と `throughput` はトイベンチマークで同じ `f_best` に到達しました
- `balanced` は `throughput` より少ない partial step でそれを達成しました
- `conservative` はデフォルトとするには、慎重さと引き換えに解の品質を犠牲にしすぎていました

## Rust ツールチェーン

Rust 1.94.0 でテストを実行します。

```bash
cargo +1.94.0 test
```

## 小規模比較ベンチ

小規模比較ベンチを実行します。

```bash
cargo +1.94.0 bench --bench preset_compare
```

このカスタムベンチターゲットは、各プリセットの経過時間と主要なエンジン統計情報を CSV 形式の行で出力します。
明示的な前後比較は現在以下のペアで行われます。

- `legacy_baseline` vs `balanced`

このペアは「旧動作 vs 推奨デフォルト」の比較として最も有益です。

## 軽量レポート

軽量レポート（シングルショットのタイミング + エンジン統計情報）を実行します。

```bash
cargo +1.94.0 run --release --example preset_report
```

生成された CSV を使用して、少なくとも以下の項目を比較してください。

- `truth_evals`
- `partial_steps`
- `invalid_eval_rejects`
- `f_best`

優れたデフォルトプリセットは、`throughput` の `f_best` 改善の大部分を維持しつつ、`partial_steps` を `throughput` よりも十分に低く抑えるべきです。


## Objective pruning パラメータ

Objective pruning は `EngineConfig` とプリセットを通じて設定可能です。現在のプリセットでは、balanced / throughput / conservative の動作を区別するために以下のゲートを使用しています。

- `objective_prune_min_smc_rank`: 到達する必要がある、異なる SMC レベル間での1始まりのランクです
- `objective_prune_min_level`: プルーニングが発動する前に必要な、1始まりの最小ラダーレベルです
- `objective_prune_require_back_half`: true の場合、プルーニングはラダーの後半にさらに制限されます
- `objective_prune_disable_for_audit`: true の場合、監査が必要な候補は objective pruning をバイパスします

推奨される解釈は以下の通りです。

- `balanced`: 中程度のプルーニング。2番目の SMC ランクおよびレベル2から開始します
- `throughput`: 早期プルーニング。1番目の SMC ランクから開始します
- `conservative`: 遅延プルーニング。より遅い段階で、かつ後半のみで開始します
