# 貢献する

<p align="center">
  <a href="CONTRIBUTING.md">English</a> · <a href="CONTRIBUTING.ko.md">한국어</a> · <a href="CONTRIBUTING.zh-CN.md">简体中文</a> · <strong>日本語</strong>
</p>

コントリビューションでは、製品の中心的なルールを維持する必要があります。つまり、測定されたリスクにより最小の可逆的な制限が必要になるまで有用な作業を実行し続け、不確実な所有権をプロセスを一時停止する権限として使用しないでください。

変更を送信する前に、次を実行します。

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
bash tests/run.sh
```

Windows 固有の変更では、`powershell -File .\tests\run.ps1` も実行する必要があります。 `docs/` でのドキュメントの変更には、英語 `.md`、韓国語 `.ko.md`、簡体字中国語 `.zh-CN.md` および日本語の `.ja.md` ファイル、有効な相対リンク、および公的利用に関係のない個人パス、認証情報、または内部文書はありません。

ユーザーに見える動作と実行された検証を説明する、焦点を当てた問題またはプル リクエストを開きます。 セキュリティ関連のレポートは、[SECURITY.md](SECURITY.ja.md) に記載されている非公開の脆弱性報告フォームに属します。
