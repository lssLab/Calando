# 適応制動距離

<p align="center">
  <a href="stopping-distance.md">English</a> · <a href="stopping-distance.ko.md">한국어</a> · <a href="stopping-distance.zh-CN.md">简体中文</a> · <strong>日本語</strong>
</p>

Memory Supervisor はメモリ使用量を低く抑えるように設計されていません。 既存の作業は無制限のままですが、headroom は安定した速度で下落しています。 新しい作業から開始され、測定された軌跡が実際のリスクに近づいた場合にのみ段階的な保護が適用されます。

<a id="calculation"></a>
## 計算

```text
reaction time       = max(3 seconds, 5 × sensor interval)
recovery floor      = 0.5% of detected capacity, bounded to 256–1024 MiB
corroborated rate   = the largest sustained physical, commit, or tracked-work rate
                      (the same growth is counted only once)
recovery reserve    = recovery floor + corroborated rate × reaction time
                      (capped at 25% of detected capacity)
new-work floor      = recovery reserve + one minimum work block
                      (capped at 30% of detected capacity)
```

パーセンテージは、ノイズの多い測定によってリザーブが膨らむのを防ぐだけです。 リアルタイムの決定では、使用可能なメモリ、その持続的な変化率、ネイティブ オペレーティング システムの負荷、および回復予備量に達するまでの推定時間を組み合わせます。

- `ALLOW / OBSERVE`: 既存の作業と提案された作業は引き続き制限されません。
- `HOLD`: およそ 2 つの反応ウィンドウが残っているか、新しい最小ワーク ブロックが 1 つ入る余地がない場合、新しい拡張のみが待機します。
- `DRAIN`: 確認されたモーションが 1 つの反応ウィンドウ内でリザーブに達すると、属性のあるエージェントに属する将来の作業のみが必要最小限の量だけ削減されます。
- 外部からの圧力や原因不明の圧力によって、AI プロセスが恣意的に停止されることは決してありません。
- ローカル プロセスの一時停止は最後の安全策であり、初期の段階で成長を止めることができず、正確なターゲットが確認された後にのみ使用されます。

したがって、使用率が高いだけではブレーキがかかることはありません。 低速で安定したワークロードが継続します。 急激な落下では、測定された速度によって要求される停止距離が大きくなります。

<a id="controlled-physical-machine-verification"></a>
## 制御された物理マシンの検証

| アイテム | 環境 |
| --- | --- |
| ホスト | Windows 11 Pro、15.73 GiB RAM、Intel i5-1135G7、8 論理 CPU |
| ゲスト | WSL2 Ubuntu、x86-64 |
| テストされたカーネルによって検出された容量 | 7,941 MiB |
| スワップ | 16 GiB |
| AIツール | Claude Code 2.1.217 および Codex CLI 0.145.0 |
| Supervisor | Rust計測用ビルド、センサー間隔は1秒、ユーザーメモリ上限はオフ |

AI プロセス ツリーの外側にある制限付きアロケーターは、約 64 MiB/s で実メモリにアクセスし、利用可能な 1 GiB を下回る約 32 MiB/s まで減速し、350 MiB で停止し、20 秒間保持してからすべてを解放しました。 外部プログラムがプレッシャーを生み出したため、Claude Code や Codex を責めずに新しい作業にブレーキをかけるのが正しい行動でした。

| ポイント | 検証結果 |
| --- | --- |
| 始める | 5,910 MiB が利用可能。 既存の作業は無制限 |
| 最初のブレーキ | `HOLD`、1,143 MiB 利用可能、577.6 MiB 予約、8.8 秒で予約 |
| 次のブレーキ | `DRAIN`、530 MiB 利用可能、409.6 MiB 予約、予約まで 3.9 秒 |
| `DRAIN`中 | 新しい subagent 開始が延期されました。 進行中の編集は許可されています |
| 最低点 | 約 350 MiB が利用可能。 端末のフリーズや強制終了はありません |
| 帰属 | 外圧; エージェントが制限されておらず、PID が一時停止されていない |
| 回復 | リリース後は 5,902 MiB が利用可能。 安定期間後に新しい作品が再開されました |

<a id="scale-verification"></a>
## スケール検証

決定論的な Rust テストでは、512 MiB から 10 TiB の容量と 1 MiB/s から 128 GiB/s の持続的な低下の同じ時間関係が維持されます。 リザーブまで 12 秒では `DRAIN` は入力できません。7 秒では `HOLD` に、4 秒では `DRAIN` に入力できます。 マルチエージェント テストでは、各制御間隔が残りのステージから必要最小限のターゲットのみを選択し、軌道が改善され次第次の制限を停止することも検証します。

<a id="scope-limits"></a>
## 範囲の制限

- 物理的境界付近のテストでは、1 つの Windows + WSL2 環境での外部圧力と自動回復をカバーします。
- 大規模なエージェント フリートと極端なメモリ サイズは、決定論的シミュレーションで検証されます。
- プロセスを一時停止すると、それ以上の増大は停止しますが、すでに使用しているメモリはすぐには戻りません。
- パブリック Windows バイナリが信頼できるコード署名を取得するまでは、Windows セキュリティ設定によって実行がブロックされる可能性があります。 [Windows 実行可能ファイルの信頼](../guides/windows-signing.ja.md)を参照してください。
