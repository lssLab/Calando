# ドキュメント

<p align="center">
  <a href="README.md">English</a> · <a href="README.ko.md">한국어</a> · <a href="README.zh-CN.md">简体中文</a> · <strong>日本語</strong>
</p>

すべての文書を読む必要はありません。 この 3 つのうちのやりたいことに当てはまるものから始めてください。

| ここから始めましょう | 必要なときに読んでください |
| --- | --- |
| [インストール、接続、サポート環境](guides/setup.ja.md) | 初めてインストールするか、実行中の Claude Code または Codex セッションに接続するか、フックの信頼性と Windows、WSL2、macOS、または Linux の状態を確認します。 |
| [Memory Supervisor の仕組み](guides/how-it-works.ja.md) | 段階的なブレーキング、CLI と Codex App、blind control、またはフェデレーションについて理解する |
| [操作、通知、および回復](guides/operations.ja.md) | ステータスおよび制御コマンドを使用したり、通知を設定したり、一時停止したプロセスや回復を処理したりできます。 |

元の詳細な README を最初から最後まで続けて読むには、[詳細ガイド](detailed-guide.ja.md) を使用してください。

<details>
<summary><strong>すべての専門家の参考資料を表示</strong></summary>

<a id="architecture-and-platforms"></a>
### アーキテクチャとプラットフォーム

- [アーキテクチャ](guides/architecture.ja.md) — 端末、エージェント、フック、およびスーパーバイザ プロセス
- [Codex Desktop App](guides/codex-app.ja.md) — 共有 App Server 内での会話ごとの監視と制御
- [フェデレーション](guides/federation-topology.ja.md) — 1 台のマシン上の複数のカーネルと端末を調整します。
- [プラットフォーム](guides/platforms.ja.md) - Windows、WSL2、Linux、macOS、VM、コンテナ
- [リソース境界](guides/resource-boundaries.ja.md) — 自動しきい値、オプションの上限、および回復境界

<a id="connections-and-operations"></a>
### 接続と操作

- [Claude Code](guides/usage-claude.ja.md) — Claude Code フックと接続検証
- [Codex](guides/usage-codex.ja.md) — Codex CLI およびデスクトップ アプリのフックと信頼
- [通知](guides/notifications.ja.md) — 端末、オペレーティング システム、Discord、および Telegram
- [Windows 実行可能ファイルの信頼](guides/windows-signing.ja.md) - 未署名の Windows ビルドと Smart App Control

<a id="security-performance-and-verification"></a>
### セキュリティ、パフォーマンス、検証

- [セキュリティ](guides/security.ja.md) — 観察されたデータ、管理範囲、および決して処理されないデータ
- [パフォーマンス](guides/performance.ja.md) — 常駐メモリとフック/ステータス レイテンシ
- [テストカバレッジ](testing/test-matrix.ja.md) — 公開テストの対象となる動作とプラットフォーム
- [適応停止距離](testing/stopping-distance.ja.md) — ブレーキの計算と制御された測定

</details>

すべての公開文書は、英語`.md`、韓国語`.ko.md`、簡体字中国語`.zh-CN.md`、日本語`.ja.md`で提供しています。
