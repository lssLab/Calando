# Codex アダプター

<p align="center">
  <a href="README.md">English</a> · <a href="README.ko.md">한국어</a> · <a href="README.zh-CN.md">简体中文</a> · <strong>日本語</strong>
</p>

通常は最上位の 1 行インストーラーを使います。更新は自動で行われ、すぐに更新したいとき、または後から Codex を追加したときは `calando update` を実行します。どちらもアダプターをアトミックにマージし、関係のない hooks はそのまま残します。 `hooks.json.template` は手動検査とカスタム展開用です。 `__MEMORY_SUPERVISOR_ROOT__` をスラッシュの絶対パスに置き換え、`__CODEX_HOOKS__` をその Codex ホームの絶対パス `hooks.json` に置き換えます。 ソース パスにより、別の Codex ホームがプロジェクト hook として再検出した場合、ゲートはこのユーザー hook を無視できます。

Codex hook JSON は、SessionStart、UserPromptSubmit、SubagentStart、SubagentStop、Stop、PreToolUse、および PostToolUse に対して Claude と互換性があります。 SubagentStop は、モデル コンテキストを追加せずにログに記録されます。 Codex には PostToolBatch がないため、PostToolUse は同じ遷移通知パスを呼び出します。

アダプターには、Codex 0.145.0 以降と `hooks stable true` が必要です。 ネイティブ `PreToolUse` は `spawn_agent` を `Agent` にマップするため、通常の `codex`、`codex exec`、および IDE ホストのセッションは同じ事前割り当てゲートを使用します。 サポートされていないリリースは変更されないままになります。 [Codex の使い方](../../docs/guides/usage-codex.ja.md)を参照してください。
