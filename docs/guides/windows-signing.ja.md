# Windows 実行可能ファイルの信頼

<p align="center">
  <a href="windows-signing.md">English</a> · <a href="windows-signing.ko.md">한국어</a> · <a href="windows-signing.zh-CN.md">简体中文</a> · <strong>日本語</strong>
</p>

Windows 実行可能ファイルは現在、[SignPath Foundation](https://signpath.org/) によるオープンソースのコード署名レビュー中であるため、レビューが完了するまで Windows 11 では Smart App Control をオフにしておく必要があります。 現在の準備ビルドには Authenticode 署名が含まれていません。 インストーラーは、リリースで公開された SHA-256 チェックサムを検証しますが、整合性検証は、Windows が予期する発行者の署名を置き換えるものではありません。

<a id="when-this-applies"></a>
## これに該当する場合

- PowerShell、Windows ターミナル、および Windows ネイティブの Codex App サーバーはネイティブの Windows パスを使用し、Smart App Control の影響を受けます。
- Codex App ウィンドウが Windows 上にあり、その App Server とツールが WSL 内で実行されている場合は、WSL Supervisor をインストールします。 Windows 実行可能ファイルは起動されないため、このガイダンスは適用されません。
- 組織アプリ制御、Windows 11 S モード、および個別の SmartScreen ダウンロード レピュテーション チェックにより、他の制限が課される可能性があります。 インストーラはそれらをバイパスしません。

<a id="current-installation-condition"></a>
## 現在の設置状況

Smart App Control にはアプリケーションごとの例外はありません。 署名されていないネイティブ Windows ビルドを使用するには、**Windows セキュリティ → アプリとブラウザーのコントロール → Smart App Control** を確認します。

| ウィンドウの状態 | 結果 |
| --- | --- |
| 64 ビット Windows 10 | Smart App Control は使用できないため、SAC 設定は必要ありません。 SmartScreen が表示された場合は、ダウンロードがこのリポジトリのリリースからのものであることを確認してください。 |
| Smart App Control はすでに `Off` です | ネイティブ インストールを続行します。 別の SmartScreen プロンプトが表示された場合は、ダウンロードがこのリポジトリのリリースから行われたものであることを確認してください。 |
| 現在の Windows 11 ビルドには、コントロールの再有効化が表示されます | 未署名のビルドを使用する場合は、これを `Off` に設定します。 後で同じ画面から再度有効にすることができます。 |
| Windows 11 では再有効化コントロールが表示されません | オフにすると、再度オンにするために Windows のリセットまたは再インストールが必要になる場合があるため、最初に確認してください。 |
| S モードの Windows 11 またはブロックする組織ポリシー | ネイティブ Windows パスはサポートされていません。 インストーラは制限を回避できません。 必要に応じて、WSL などの別途許可された環境を使用してください。 |

`Win + R` から `winver` を実行して、Windows のバージョンとビルドを検査します。 再有効化コントロールは Windows 11 24H2 ビルド 26100.8117 以降および 25H2 ビルド 26200.8117 以降で展開されているため、Smart App Control をオフにする前にコントロールが実際に表示されていることを確認してください。 現在の基準については、Microsoft の [Smart App Control FAQ](https://support.microsoft.com/en-US/Windows/Security/Threat-Malware-Protection/smart-app-control-frequently-asked-questions) および [ロールアウト ノート](https://support.microsoft.com/en-au/help/5079391) を参照してください。

<a id="verifying-a-download"></a>
## ダウンロードの検証

1 行のインストーラーは、リリース ソースと実行可能ファイルをダウンロードし、公開されている SHA-256 値を自動的に検証します。 手動でダウンロードした実行可能ファイルの場合は、PowerShell でその署名の状態を検査します。

```powershell
Get-AuthenticodeSignature .\memory-supervisor.exe | Format-List Status, StatusMessage, SignerCertificate
```

`NotSigned` は、この準備ビルドの予想される結果です。 リリース アーティファクトがコード署名されると、インストール ガイドとリリース ノートにその変更が記載され、この条件もそれらとともに更新されます。
