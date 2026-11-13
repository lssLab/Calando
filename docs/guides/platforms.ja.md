# プラットフォームの展開と federation

<p align="center">
  <a href="platforms.md">English</a> · <a href="platforms.ko.md">한국어</a> · <a href="platforms.zh-CN.md">简体中文</a> · <strong>日本語</strong>
</p>

<a id="one-supervisor-per-protected-user-and-pid-control-environment"></a>
## 保護されたユーザーおよび PID 制御環境ごとに 1 つの supervisor

supervisor は、OS ユーザーと PID 名前空間に表示されるプロセス インベントリを読み取ります。 1 つのインストールでは、Windows ターミナル、iTerm、VS Code、tmux、SSH、または別のターミナル サーフェスから開かれたかどうかに関係なく、同じ PID 制御環境内のユーザーの Claude Code および Codex セッションがカバーされます。

supervisor は、ローカル PID 制御環境の外に信号を送信することはありません。 ホストに 1 回インストールし、保護する必要があるすべての WSL ディストリビューション、VM、または PID 分離コンテナーに 1 回インストールします。 WSL 2 ディストリビューションは、個別の PID 名前空間を保持しながら、マネージド VM とカーネルを共有できるため、個別のローカル インスタンスが必要になります。 各インスタンスは、小さな状態スナップショットを共有 federation ディレクトリに公開します。 Hooks は、新しいファンアウト admission に過去 10 秒間の最も悪い有効なスナップショットを使用しますが、ローカル supervisor のみがローカル PID を一時停止できます。

| ベースOS | その上の環境 | 必要なインストール | Federation 境界 |
| --- | --- | --- | --- |
| 窓 | 1 つ以上の WSL2 ディストリビューション | Windows とすべての WSL ディストリビューション | 各 WSL インスタンスは Windows ユーザーの `.calando/instances`（混在期間中は `.memory-supervisor/instances` も）を自動検出します |
| Windows、macOS、または Linux | ダイナミック メモリ VM | ホストとゲスト全員 | ホストローカル共有フォルダーを介して、実際に同じ物理 RAM をめぐって競合する側のみを接続します。 |
| Windows、macOS、または Linux | 固定メモリ VM | ホストとゲスト全員 | それぞれの側を独立させてください。 固定割り当て境界を越えてフェデレーションしないでください |
| Linux カーネル (ネイティブ Linux、WSL、またはデスクトップ VM) | 1 つ以上の PID 分離コンテナ | カーネルホスト環境とすべての分離コンテナ | そのカーネル内のホストローカルボリュームを共有する |
| ネストされた任意の組み合わせ | 保護されたすべての PID 名前空間 | 動的共有メモリ境界ごとに 1 つの接続 | 1 つのディレクトリを固定 VM 境界またはネットワークを越えて拡張しないでください。 |

<a id="codex-app-follows-the-app-server-environment"></a>
### Codex App は App Server 環境に従います

Codex App ウィンドウとその実行エンジンは、同じオペレーティング システム環境で実行する必要はありません。 Calando は、デスクトップ ウィンドウではなく、`codex ... app-server` プロセスに従います。

- WSL エンジンを使用する Windows Codex App は、その WSL ディストリビューションにインストールされている Supervisor によって保護されます。 WSL App Server を検出し、そのプロセスのアクティブな `CODEX_HOME` を解決し、その論理スレッド、hook の決定、WSL child ツール、および WSL 側の物理ブレーキを管理します。 これには、署名のないネイティブ Windows Supervisor または Smart App Control の変更は必要ありません。
- その WSL インスタンスは、Windows アプリ UI プロセスや別の Windows ネイティブ Claude Code または Codex CLI を測定したり一時停止したりすることはできません。 Windows プロセスをカバーする必要がある場合は、Windows Supervisor もインストールします。 Windows インスタンスと WSL インスタンスは、ローカル PID 制御を維持しながら、admission から federation を共有します。
- ネイティブ Windows または macOS App Server は、その OS で Supervisor を使用します。 Linux、別の WSL ディストリビューション、VM、または PID 分離コンテナーで実際に実行されている App Server は、その環境内で Supervisor を使用します。 リクエストを行ったウィンドウまたはクライアントが他の場所にある場合でも、同じルールが適用されます。
- 固定メモリ VM またはリモート コンピューターは、それ自体を独立して保護します。 同じ物理 RAM を動的に競合する実行環境のみをフェデレーションします。

これは、ハードコーディングされた Windows/WSL 例外ではなく、一般的なプロセス境界ルールです。 共有 Windows/WSL `CODEX_HOME` は、ファイル レイアウトのケースとしてのみ処理されます。 hook ファイルは両方のネイティブ コマンド フィールドを保持しますが、各コマンドは依然として独自の環境内の Supervisor と PID にのみ到達します。

共有パスがない場合でも、各インスタンスは独自のローカル環境を保護します。 クロス環境 admission と結合された `calando status` ビューのみが使用できません。

federation リーダーは Windows/WSL ペアをホワイトリストに登録しません。 1 つのホストとローカルのメモリ境界内の Windows、WSL、Linux、および macOS ピアは同じスナップショット コントラクトを使用し、最後の 10 秒間の最も厳密で有効な新しい作業の決定のみが適用されます。 Windows/WSL が特別なのは、共有パスを自動的に検出できるという点だけです。 macOS および Linux ホスト、動的 VM、コンテナー、およびネストされた環境は、実際の境界として共有フォルダーを使用します。 同じホスト名のクローンゲストまたはコンテナに一意の `MEMORY_SUPERVISOR_INSTANCE` 値を与えます。

`CODEX_HOME` ではなく、federation ディレクトリを共有します。 Hook ファイル、信頼状態、および PID 権限は、各環境に対してローカルに維持されます。 Windows アプリと WSL ランタイムが実際に同じ `CODEX_HOME` を使用する場合にのみ、1 つの Codex ファイルが Windows と POSIX の両方のコマンド フィールドを保持します。 これは hook ファイル レイアウトの例外であり、federation OS の組み合わせの制限ではありません。

<a id="runtime-and-startup"></a>
## ランタイムとスタートアップ

パブリック リリースのインストールでは、Git、Python、または Rust を必要としたり、インストールしたりする必要はありません。 同じリリースから現在の OS とアーキテクチャのソース バンドルとネイティブ バイナリをダウンロードし、両方の SHA-256 値をチェックします。 貼り付けたコマンドのダウンローダーに加えて、オペレーティング システムの標準アーカイブと SHA-256 サポートを使用します。 手動開発チェックアウトは、Rust 1.88 以降を使用してローカルに構築できます。

Windows 10 には Smart App Control がありません。 Supervisor 実行可能ファイルの [最小 Windows ベースライン](https://doc.rust-lang.org/rustc/platform-support/windows-msvc.html) も Windows 10 であり、必要なメモリとプロセス機能は Windows 10 で利用できます。 SAC 設定は必要ありませんが、SmartScreen プロンプトではダウンロード ソースを確認する必要があります。 Windows 11 Smart App Control は、チェックサムが正しい場合でも新しい署名されていない実行可能ファイルをブロックすることができ、アプリごとの例外はありません。 したがって、パブリック Windows アーティファクトが署名されるまで、実行可能ファイルがインストールされて実行されている間、ネイティブ Windows 11 パスは Smart App Control をオフにしておく必要があります。 Windows インストーラーはカットオーバーの前に候補を実行し、Windows が拒否した場合は既存のサービスをそのまま残します。 Windows 11 24H2 ビルド (26100.8117 以降) および 25H2 ビルド (26200.8117 以降) は、可逆的なオン/オフ コントロールを受け取ることができますが、ロールアウトは段階的に行われます。SAC をオフにする前に、`winver` をチェックし、再有効化コントロールが表示されていることを確認してください。 そのコントロールのない古いビルドまたはデバイスでは、コントロールをオンに戻すためにリセットまたは再インストールが必要になる場合があります。 WSL バイナリでは Windows Smart App Control を変更する必要はありませんが、WSL 内のプロセスのみを保護します。 S モードの Windows 11 と、依然として実行可能ファイルをブロックする組織のアプリ制御ポリシーは、ネイティブ パスをサポートしていません。 [Windows 署名 Runbook](windows-signing.ja.md)、Microsoft の [Smart App Control FAQ](https://support.microsoft.com/en-US/Windows/Security/Threat-Malware-Protection/smart-app-control-frequently-asked-questions)、[ロールアウト ノート](https://support.microsoft.com/en-au/help/5079391)、[コード署名ガイダンス](https://learn.microsoft.com/windows/apps/develop/smart-app-control/code-signing-for-smart-app-control)。

| プラットフォーム | ユーザーレベルの起動メカニズム |
| --- | --- |
| Linux / WSL | `~/.config/systemd/user/calando.service`; インストーラ所有の残留物（利用可能な場合） |
| macOS | `~/Library/LaunchAgents/io.github.lsslab.calando.plist` |
| 窓 | `Calando` ユーザーログオン時のスケジュールされたタスク |
| ユーザー systemd のない Unix | PID 監視フォールバック。 すぐに起動しますが、ブート起動は手動です |

`calando update` は、可能な場合はチェックアウトを更新し、ネイティブ ランタイムを検証してアクティブ化し、ローカル サービスをリロードし、検出されたすべてのサポートされている CLI を再接続します。 daemon カットオーバー中にエージェント PID を送信することはなく、新しい daemon は一時停止された ID を `runtime.json` からリロードします。

既定では自動更新がオンです。daemon は 1 日 1 回最新リリースを確認し、一時停止中のものがなくメモリが落ち着いているときに、自分のサービスの外で更新を実行します（忙しいときは 1 時間ごとに再試行）。Codex が接続されていて hook 定義が変わるリリースは自動適用せず、`calando update` と hook の信頼を案内します。`calando update auto off` で自動更新を止められます。更新の間にフェールオープン保護の短いギャップが生じることがあります。 更新後は必ず `calando status` を実行してください。 実際の Codex hook 定義の変更のみ、ユーザーがその CLI プロセスの `/hooks` またはデスクトップ アプリの **設定 → Hooks** を個人的に再度信頼する必要があります。 再起動が信頼に代わることは決してありません。 アプリ設定を変更すると、共有 App Server によってロードされた既存のタスクが更新されますが、個別の CLI プロセスは更新されません。 Claude Code には、hook ごとのハッシュ承認がありません。ユーザー設定 hook は、Codex のような項目レビュ​​ーなしでアクティブですが、対話型クロードは、ユーザーが現在のフォルダーまたはその親の 1 つに対するワークスペースの信頼を受け入れるまで、すべての設定ファイル hooks を保持します。 信頼された実行セッションは通常、その後のユーザー設定の変更を自動的に再読み込みします。 これらの信頼境界とリロード境界は、daemon 自体の再起動よりも長く続く可能性があります。

サポートされているランタイムはネイティブ Rust バイナリです。 supervisor を再起動すると、永続化された状態が再ロードされますが、インストールされたビルドの置き換えは、可能であればアクティブな CLI セッションの間に行う必要があります。

<a id="what-happens-after-a-machine-restart"></a>
### マシンの再起動後に何が起こるか

- Linux と WSL は有効なユーザー ユニットを使用します。 インストーラ所有のリンガーにより、ユニットはユーザー マネージャーから開始できます。 WSL サービスは、WSL の配布自体が開始された後にのみ開始されます。
- macOS は、GUI ログイン時に `RunAtLoad` および `KeepAlive` LaunchAgent をロードします。
- Windows はユーザーのログオン時にスケジュールされたタスクを開始し、予期しない daemon の終了を 1 分間隔で最大 5 回再試行します。 このタスクはコンソールの切り離しを要求します。これは、コンソールが daemon プロセスに単独で属している場合にのみ、daemon によって実行されます。 したがって、バックグラウンドの開始では、ユーザーに面した黒いウィンドウが開いたままになり、定期的な PowerShell センサーは `CREATE_NO_WINDOW` を使用します。 既存の端末から実行されるコマンドは、その共有端末を維持します。
- Claude Code および Codex hook/skill ファイルはインストールされたままになります。 ログイン後に新しい AI CLI セッションを開き、`calando status` を実行します。 実際の hook ハッシュ変更のみレビューが必要です。Codex CLI で `/hooks` を使用し、Codex App で **設定 → Hooks** を使用します。
- 再起動はアップデートではありません。 ソース、セットアップ、または後でインストールされた CLI を再適用する必要がない限り、`calando update` を実行しないでください。

<a id="federation-paths"></a>
## Federation パス

- デフォルト: `~/.calando/instances`
- オーバーライド: `MEMORY_SUPERVISOR_FEDERATION_DIR`
- 永続化ポインタ: `~/.calando/federation-dir`
- 状態ポインタ: `~/.calando/state-dir`
- WSL は、Windows ユーザーの共有インスタンス ディレクトリを自動的に検索します。
- WSL のデフォルトのインスタンス名には `WSL_DISTRO_NAME` が含まれるため、WSL が Windows ホスト名を共有している場合でも、同じホスト上の Ubuntu と Debian は相互に上書きしません。
- 古いスナップショット、不正な形式、またはエラーのあるスナップショットは、admission には参加しません。
- WSL 以外のクローン ゲストが依然として ID を共有している場合は、`MEMORY_SUPERVISOR_INSTANCE` を一意の名前に設定します。

```bash
calando status
```

Federation はグローバル バックプレッシャーであり、スケジューラではありません。 workers を移行することも、別の OS が所有する PID にシグナルを送信することもありません。

<a id="multiple-sshtmux-sessions-and-vps-deployments"></a>
## 複数の SSH/tmux セッションと VPS 導入

1 つのユーザーレベルのインストールは、同じ PID 制御環境内の SSH ログイン、ターミナル ウィンドウ、および `tmux` ペインにわたるユーザーの Claude Code および Codex セッションをカバーします。 彼らは、独立した監督者を通じて競合するのではなく、1 つの admission の決定を共有します。 マルチユーザー サーバーは、保護された OS ユーザーごとに 1 回インストールする必要があります。 `hidepid` などの `/proc` の制限により、あるユーザーが別のユーザーのプロセスを検査することができなくなりますが、製品はその境界をバイパスしません。

ネイティブの cgroup 上限、PSI、スワップ/再利用、およびすべての同じユーザーのリモート セッションが同じポリシーをフィードするため、制約付き VPS は自然な展開形態です。 インストールされているユーザー サービスを有効にし、必要に応じてユーザーを待機させて、SSH シェルを開いていない状態でもサービスを利用できるようにします。 hook とターミナルのメッセージは常に届き、外出先で受け取るには Telegram を使います。 このパスは Linux と cgroup の契約テストでカバーされていますが、数時間にわたる実際の VPS モデルのソークが完了したとはまだ主張されていません。

<a id="native-capacity-and-sensors"></a>
## ネイティブ容量とセンサー

| プラットフォーム | 容量と使用可能なメモリ | プレッシャーとプロセス |
| --- | --- | --- |
| Linux / WSL | `/proc/meminfo` は、それを囲むすべての cgroup v1/v2 上限によって制限されます | OS のメモリ不足信号 (PSI、再利用、スワップ、およびメモリ不足カウンタ)、`/proc/<pid>`、PID 開始ティック、TTY ID |
| macOS | `sysctl hw.memsize`; `vm_stat` の空き/非アクティブ/パージ可能なページ | 公開時のカーネル圧力レベル、プライマリ `vm_stat` ページアウト/圧縮傾向、`ps` 開始時間および TTY |
| 窓 | `GlobalMemoryStatusEx` 物理メモリ | `GetPerformanceInfo` コミット headroom、キャッシュされた CIM プロセス インベントリ、作成 ID、コンソール/ConPTY 証拠 |

Linux は、無制限のリーフを信頼するのではなく、すべての cgroup の祖先をチェックします。 macOS の圧力レベル sysctl を読み取れない場合、`vm_stat` カウンターは利用可能なままですが、ネイティブ圧力は不明/信頼性が低いとして報告され、圧力センサーのエラーが明らかになり、admission は保守的に保持されます。 故障した `vm_stat` も、実際のセンサーの故障です。 匿名 RSS は同じ形式で公開されないため、macOS はプロセスごとの近似値として RSS を使用します。 Windows は、安価なグローバル カウンターをティックごとに更新し、高価なプロセス インベントリを 3 秒間キャッシュします。

すべてのプラットフォームは、`sensor_ok`、`sensor_errors`、および `last_process_scan_ts` をレポートします。 プロセス スキャンが失敗すると、最後のインベントリが診断用に表示されたままになる場合がありますが、その古いインベントリによって新たなリークの一時停止や一時停止された PID 調整が発生することはありません。

アダプティブ admission は、実際の headroom、短い/長い勾配、疲労までの時間、ネイティブの遭難、最近のバースト、および自動回復可能リザーブを使用します。 固定パーセンテージの RAM は予約されません。 安定した高使用はオープンのままにすることができます。 十分なheadroomを伴う急速な下落がホールドされる前に観察され、ホールドはリザーブに近い、持続的な短いTTE、`calando cap` の上限、または劣化した保護のために予約されます。

<a id="wsl2-capacity-on-a-16-gib-windows-host"></a>
## 16 GiB Windows ホスト上の WSL2 容量

Microsoft は現在、WSL2 のデフォルトの `memory` の上限を Windows RAM の 50% と文書化しています。 したがって、16 GiB ホストでは、明示的な `memory=8GB` 行を削除すると、通常、負荷の高い Linux CLI セッションにより多くの余地が与えられるのではなく、同じ 8 GiB の上限が残ります。 `memory=10GB` は、Windows アプリケーションと並行したいくつかの重い WSL タスクの例です。 `memory=12GB` は、Windows 側のワークロードが軽い場合にのみ考慮すべき大きな例です。 どちらも supervisor のデフォルトまたは自動推奨ではありません。

```ini
[wsl2]
memory=10GB
swap=16GB

[experimental]
autoMemoryReclaim=gradual
```

`memory` は最大値であり、10 GiB の事前割り当てではありません。 supervisor には、依然として VM の上限が必要です。これは、正確な PID 一時停止によって以降の実行が停止されますが、常駐メモリがすぐに返されず、無関係な Linux または Windows アプリケーションが制御されないためです。 WSL の上限が高くなると、エージェント headroom が増加しますが、外部アプリに対するホストの最悪の場合の予備は減少します。 federation は両方の側を監視しますが、一方のカーネルの PID 信号をもう一方のカーネルのメモリ再利用に変換することはできません。

`.wslconfig` への変更を有効にするには、WSL VM を停止する必要があります。 `wsl --shutdown` はアイドル境界でのみ実行してください。これは、実行中のすべての WSL ディストリビューションとその中のすべての CLI セッションが即座に終了するためです。 Microsoft の [高度な WSL 設定](https://learn.microsoft.com/windows/wsl/wsl-config) および [`wsl --shutdown` コマンド](https://learn.microsoft.com/windows/wsl/basic-commands#shutdown) を参照してください。

<a id="optional-local-cli-memory-budget"></a>
## メモリ上限（上級）

上限は**既定ではオフ**です。この制御環境に見えるすべての Claude Code と Codex のツリーにかかる合計の上限で、CLI ごとの制限や Windows + WSL をまとめた枠ではありません。

```bash
calando cap 6
calando cap off
```

`6` は GB 指定の例です。Windows、WSL、各 VM、各分離コンテナーでそれぞれ実行します。コマンドは説明を表示して `[y/N]` で確認し、対話できない呼び出し元は `--yes` を付けます。値はこの環境で今使える量と照らして検証します。上限に近づくと新しい重い作業を待たせ、上限を超えると増え続けているサブエージェントを一つ一時停止します。一時停止しても使用済みのメモリは戻らず、使用量が一時的に上限を超えることもあるため、バイト単位の上限が必要なら cgroup/コンテナー/VM の制限を使います。通常の利用には勧めず、厳密な研究やベンチマーク向けです。

<a id="persistent-advanced-settings"></a>
## 永続的な詳細設定

通常の操作には構成ファイルは必要ありません。 高度なオーバーライドは `~/.config/calando/config.json` にあります。 同じ名前の環境変数が優先されます。 上限の設定と解除には `calando cap` を使います。

```json
{
  "MEMORY_SUPERVISOR_TICK_S": 1,
  "MEMORY_SUPERVISOR_WINDOWS_PROCESS_SCAN_S": 3,
  "MEMORY_SUPERVISOR_CLI_HARD_CAP_MB": 32768
}
```

`MEMORY_SUPERVISOR_TICK_S` は 0.25 ～ 5 秒を受け入れます。 5 秒の上限により、次のサンプルは 10 秒の状態鮮度契約と 5 秒のリース契約内に保持されます。 範囲外の値は 1 秒に戻り、`configuration_error` に表示されます。

`MEMORY_SUPERVISOR_DIR`、`MEMORY_SUPERVISOR_FEDERATION_DIR`、`MEMORY_SUPERVISOR_FORCE_PLATFORM` などのパス/ブートストラップ設定は、この JSON ファイルには属しません。 手動で高度な編集を行った後、`calando update` を実行し、`calando status` で確認します。

<a id="pause-resume-and-restart"></a>
## 一時停止、再開、再起動

- Unix の `SIGSTOP` と Windows のネイティブ プロセスの一時停止では、PID とメモリ内セッションが保持されます。
- 手動で再開するコマンドはありません。メモリ逼迫で一時停止したプロセスはメモリが回復すると自動で再開し、増加が続いて一時停止した lead は見守り付きで一度だけ自動再開します。
- lead が止まったままなら、その端末を閉じて CLI の再開機能（`claude --resume`、`codex resume`）で続けるか、`calando off` でその場で再開します。
- 制御意図は信号の前に保持され、daemon 確認応答後にのみ完了が報告されます。
- supervisor を再起動するとインシデント台帳を読み直し、一時停止中のプロセスの管理を引き継ぎます。
- リモート インシデントは、`source` フィールドで指定された OS から制御する必要があります。

AI CLI/モデル コンテキストは、次の実際の hook 境界で配信されます。これは、オペレーティング システムの再開よりも遅くなる可能性があります。端末と Telegram への通知は別々に試行されます。

<a id="turn-the-whole-installation-on-or-off"></a>
## インストール全体をオンまたはオフにする

```bash
calando off
calando on
```

1 つの `off` コマンドは、現在の OS/PID 制御環境のサービスと自動起動を無効にし、その選択を `~/.calando/power-off` に保持します。 Claude Code と Codex hooks がインストールされ、スキルは接続されたままですが、すべての hook は静かに通過します。 `calando status` は意図的に `OFF` を報告し、`calando update` はそれを保持します。 `on` はマーカーを削除し、自動起動を復元し、新しい状態が公開されたことを確認します。

`off` はまず Calando が一時停止したものをすべて再開してから保護を切るため、一時停止したまま取り残されるプロセスはありません。管理者が `calando lock on` にしている間は `off` を拒否します。 Windows、各 WSL ディストリビューション、VM ゲスト、および PID 分離コンテナーには、個別のサービスと PID 名前空間があります。 切り替える各環境内でコマンドを 1 回実行します。

<a id="low-level-service-recovery-commands"></a>
## 低レベルのサービス回復コマンド

```bash
# Linux / WSL
systemctl --user restart calando.service
systemctl --user is-active calando.service

# macOS: restart a loaded agent
launchctl kickstart -k gui/$(id -u)/io.github.lsslab.calando

# macOS: explicitly unload, then load again
launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/io.github.lsslab.calando.plist
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/io.github.lsslab.calando.plist

# Windows
schtasks /End /TN Calando
schtasks /Run /TN Calando
```

これらのコマンドは、製品の電源スイッチとしてではなく、予期しないサービス障害を修復するために使用します。 `off` マーカーがないとサービスが利用できない場合、hooks は CLI を無効にするのではなくフェールオープンし、`calando status` は失効または欠落している supervisor とその結果生じる保護ギャップを報告します。
