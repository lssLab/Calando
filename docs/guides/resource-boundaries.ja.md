# セッション検出、容量検出、メモリ境界

<p align="center">
  <a href="resource-boundaries.md">English</a> · <a href="resource-boundaries.ko.md">한국어</a> · <a href="resource-boundaries.zh-CN.md">简体中文</a> · <strong>日本語</strong>
</p>

このガイドでは、1 つのインストールで何が確認できるか、supervisor がラッパーなしでターミナル セッションを検出する方法、OS またはゲストの使用可能なメモリを学習する方法、構成可能な各境界が実際にどのように変化するかについて説明します。

<a id="the-short-model"></a>
## ショートモデル

物理コンピューターは常に 1 つの制御境界であるとは限りません。 これには、独立して観察可能ないくつかの PID およびメモリ ドメインが含まれる場合があります。

```text
physical computer
├─ Windows host                -> Windows supervisor
├─ WSL distribution           -> Linux/WSL supervisor
├─ Linux or Windows VM        -> supervisor inside that guest
└─ Apple Silicon Mac host
   └─ macOS or Linux VM       -> supervisor inside that guest

fresh state snapshots (≤10 s) -> shared admission decision
local process table            -> only the owning instance may pause/resume that PID
```

Claude Code または Codex が実行されるすべてのホスト、WSL ディストリビューション、VM ゲスト、または PID 分離コンテナーに 1 回インストールします。 ホストのインストールではゲスト PID を通知できません。 Federation はバックプレッシャーを共有します。 カーネル間のプロセス コントローラーを作成したり、RAM の合計を加算したりすることはありません。

<a id="what-each-terminal-surface-belongs-to"></a>
## 各端子面が属するもの

| CLI が開始された場所 | 実際の観測境界 | 容量ソース | インストールして設定する場所 |
| --- | --- | --- | --- |
| PowerShell、コマンド プロンプト、またはネイティブ Windows ターミナル タブ | Windowsホスト | `GlobalMemoryStatusEx` 物理的な合計/利用可能; `GetPerformanceInfo` コミット headroom | Windows に入ると |
| WSLターミナルタブ | その WSL ディストリビューションの可視 Linux PID/メモリ ドメイン | `/proc/meminfo`、すべての囲む cgroup 制限によって絞り込まれます。 最終的には WSL VM メモリによって制限されます | サポートされている CLI を実行するすべての WSL ディストリビューション内に 1 回。 ホスト保護のために Windows にもインストールします |
| ベア Linux、SSH セッション、または tmux ペイン | Linux カーネル/PID 名前空間とユーザー権限 | `/proc/meminfo`、それを囲むすべての cgroup v1/v2 制限によって絞り込まれます | 保護された OS ユーザー/環境ごとに 1 回 |
| Apple Silicon Mac 上のターミナルまたは iTerm | macOS `arm64` ホスト | `sysctl hw.memsize`; `vm_stat` からの無料、非アクティブ、およびパージ可能なページ | macOS 上で |
| Apple Silicon Mac 上の macOS VM | ゲスト macOS `arm64` VM | ゲスト `hw.memsize` と `vm_stat`; VM 割り当てによって制限される | ゲストの中に入ると、 ホストのインストールは分離されたままになります |
| 任意のホスト上の Linux または Windows VM | ゲストOS | ハイパーバイザーの割り当てによって制限された、上記と同じネイティブ Linux または Windows ソース | ゲストの中に入ると、 ホストのインストールは分離されたままになります |
| PID絶縁コンテナ | そのコンテナの可視プロセスと cgroup ドメイン | すべての囲み cgroup 制限によって物理メモリが狭められる | 隔離されたコンテナに入ると、ホスト PID 名前空間を意図的に共有します。 |
| IntelベースのMac | macOS `x86_64` ホスト | 同じ macOS ソース | その Mac に一度入ったら |

Apple Silicon macOS VM は `arm64` のままです。 Rosetta での x86_64 の実行は互換性の対象であり、物理的な Intel Mac の検証とは区別されます。

WSL2 ディストリビューションは、別個のプロセス名前空間を保持しながら、同じ基礎となるユーティリティ VM を共有できます。 あるディストリビューションでは確実にインベントリを作成したり、別のディストリビューションの PID を通知したりできないため、CLI を実行する各ディストリビューションにインストールします。 Federation は最悪の新たな決断を下します。 共有 WSL メモリ プールの重複ビューの合計は計算されません。

<a id="how-sessions-are-discovered-without-a-wrapper"></a>
## ラッパーなしでセッションを検出する方法

ユーザーは依然として `claude` または `codex` を通常どおり起動します。 daemon はターミナル ウィンドウを列挙せず、`claude-governed`/`codex-governed` 起動コマンドを必要としません。

1. ネイティブ daemon は、その OS アカウントに表示される完全なプロセス インベントリをスキャンします。 通常の制御ループは 1 秒です。 Windows は、安価なグローバル メモリ カウンターをティックごとに読み取りながら、より高価な CIM インベントリを最大 3 秒に 1 回更新します。
2. プロセスは、その実行可能ファイルまたは最初のコマンド引数が `claude`、`codex`、または公式アーキテクチャ固有のバイナリ Codex に解決される場合、サポートされる CLI ルートです。
3. 親リンクは、ネストされたサポート対象 CLI ルートを workers としてグループ化します。 他の子孫はサポート プロセスになります。 祖先ウォークは 64 レベルに制限されているため、不正なプロセス グラフが永久にループすることはありません。
4. すべての子孫は、そのルート ツリーの RSS 推定に貢献します。 匿名メモリの 32 MiB 未満の小さな子孫は、個々の一時停止候補としてのみ省略されます。 それらはツリーの合計から削除されません。
5. PID とプロセス開始 ID により、別のプロセスを対象とした PID の再利用が防止されます。 Linux は `/proc/<pid>/stat` 開始ティックを使用し、macOS は `ps` 開始時刻を使用し、Windows は CIM `CreationDate` を使用します。
6. daemon は、lead/worker/サポート ロール、メモリ増加、ルートツリー合計、およびプラットフォームが公開する場合の検証済み端末 ID を記録します。 OS のアクセス許可、Linux `hidepid`、コンテナー、および VM の境界はバイパスされずに尊重されます。

AI CLI hooks は 2 番目のパスであり、プロセス検出器ではありません。 彼らは、新しいファンアウトの前に最新のローカル/フェデレーション状態を尋ね、次の実際の hook 境界でメイン エージェントにインシデントを注入します。 hook が欠落している場合でも、daemon はローカル プロセス テーブルを監視できますが、新しいプロセスが開始される前に AI CLI の割り当てを防ぐことはできません。 次のコマンドで両方のパスを確認します。

```bash
memory-status --connections
memory-status --all
```

<a id="how-usable-capacity-is-learned"></a>
## 使用可能な容量を学習する方法

| プラットフォーム | 容量 | 利用可能/headroom | さらなる遭難の証拠 |
| --- | --- | --- | --- |
| LinuxとWSL | `MemTotal`、最小の有限 cgroup 祖先制限に縮小 | 最小値 `MemAvailable` と各有限 `limit - current` cgroup 剰余 | PSI `some/full`、回収、スワップ、および OOM カウンター |
| macOS | `sysctl -n hw.memsize` | `vm_stat` 無料 + 非アクティブ + パージ可能なページ | 公開時のカーネル圧力レベル、ページアウト/圧縮、およびスワップの傾向 |
| 窓 | `GlobalMemoryStatusEx.totalPhys` | `GlobalMemoryStatusEx.availPhys` | コミット制限から、`GetPerformanceInfo` からコミットされたページを差し引いた値 |
| 任意の VM ゲスト | ゲスト内で報告された上記の関連行 | ゲストに表示される値。したがって、固定または動的 VM 割り当てによってすでに制限されています。 | ゲストネイティブの圧力シグナル |

解決された容量と適応ポリシーはティックごとに再計算されます。 したがって、VM の動的メモリの変更または cgroup の変更は、固定のマシン サイズ プロファイルなしで検出されます。 プライマリ センサーに障害が発生した場合、ステータスは保護機能の低下を報告し、admission が保持されます。 8 GiB フォールバック ラベルは診断用であり、マシンに実際に 8 GiB があるという主張ではありません。

supervisor は、コンテナー ランタイム、systemd ユニット、スケジューラー、または管理者が既に作成した cgroup 制限を囲む**読み取り** のみを行います。 cgroup を作成したり、CLI を cgroup に移動したり、ラッパー コマンドを必要としたりすることはありません。 そのため、通常の `claude` または `codex` の起動は引き続き検出されますが、バイト単位での cgroup 割り当ては、この製品のデフォルトのアクチュエーターではなくオプションの外部境界のままです。

supervisor はプロセスに RAM を**割り当てません**。 固定パーセンテージを確保するのではなく、停止距離を計算します。

```text
minimum breathing room = 0.5% of detected capacity, clamped to 256–1024 MiB
corroborated burn rate = max(sustained physical/commit headroom fall,
                             sustained tracked-CLI growth)
automatic reserve     = min(minimum breathing room
                            + corroborated burn rate × one reaction interval,
                            25% of detected capacity)
new-fan-out floor     = min(automatic reserve + one minimum breathing/work block,
                            30% of detected capacity)
```

物理的な headroom には追跡された CLI 割り当てがすでに含まれているため、2 つのレートは意図的に `max` と結合され、追加されて 2 回カウントされることはありません。 軌道には、少なくとも 3 つのサンプル、スパンの 1 つの反応間隔、少なくとも 60% のサポート間隔、および危険な方向へのリバウンドの少なくとも 2 倍の動きが必要です。 したがって、1 回の再利用スパイクで実際の降下を消去することはできず、1 回のバーストで実際の降下を作成することもできません。

これは車両のブレーキと同じジオメトリです。消耗が速くなると、MiB 単位での距離が長くなりますが、介入が早期に行われるわけではありません。 消耗が遅いと、同じ反応ウィンドウに到達する前に、より多くのマシンを使用できるようになります。 `HOLD` は、リザーブが 2 つの反応間隔以内であるか、新しい最小ブロックが 1 つ入る余地がない場合、新しいファンアウトのみを閉じます。 `DRAIN` は、1 つの反応間隔内でのみ、エージェント/混合属性または明示的なハード キャップを使用してのみ、段階的な既存エージェントのクッションを開始します。 1 秒ごとに適用される論理ステップの数は `ceil(remaining steps / control ticks left)` であるため、8 つの workers と数百のセッションが、エージェント数の上限が固定されていない境界で同じ最小ラダーを終了します。

安定して高い使用率を維持できます。 未処理の GREEN/YELLOW/ORANGE/RED の使用率は診断用であり、それ自体では admission を閉じたり、PID の一時停止を許可したりしません。 小型から大型の機械および窒息に近い状態で測定された証拠は、[適応停止距離](../testing/stopping-distance.ja.md)にあります。

<a id="the-five-different-boundaries"></a>
## 5つの異なる境界線

| 境界 | デフォルト | 変更方法 | 直接範囲 | 重要な副作用 |
| --- | --- | --- | --- | --- |
| 物理または VM の割り当て | OS/ハイパーバイザーのデフォルト | 物理 RAM はソフトウェアでは変更できません。 そのプラットフォームの WSL、Hyper-V、Parallels、VMware、UTM、またはクラウド VM メモリを変更する | ホストまたはゲスト OS 自体 | ゲストのメモリを増やすと、headroom の可能性が高くなりますが、ホストの最悪の場合の予備が減少します。 これを下げると、ゲストの適応しきい値と予約が下方に再計算されます。 通常、ゲストのシャットダウン/再起動が必要です。 |
| 自動検出された容量 | ネイティブセンサー | 通常は何もしません。 `MEMORY_SUPERVISOR_CAPACITY_MB` は、ネイティブ値が明らかに間違っている場合にのみ、高度なキャリブレーション オーバーライドです。 | インストールされた 1 つのインスタンス | ポリシーの計算は変更されますが、実際の OS/VM の制限は変更されません。 設定値が高すぎると安全ではありません。 低すぎると不必要に保守的になります。 |
| 適応圧力ポリシー | `balanced`; 手動の予算はありません | オプションの `protect`、`balanced`、または `performance` プロファイル、または高度なしきい値オーバーライド | インストールされた 1 つのインスタンス | Federation は、そのインスタンスのより厳格な admission の決定をピアに伝播できます。 `performance` は、実際の崩壊、保護機能の低下、または明示的な上限をバイパスすることはありません。 |
| サポートされている CLI の総メモリ バジェット (ハード キャップ) | **オフ** | その環境では `memory-supervisor budget set <GiB>` または `budget off` | すべての Claude Code および Codex ルート ツリーがその OS/PID ドメインに表示されます。 Chrome やマシン全体ではありません | キャップ付近では、新しいファンアウトが開催されます。 それを超えると、反応間隔ごとに最大 1 つの検証済み成長 worker/サポート プロセスが一時停止できます。 キャップ近接性はローカルのままです。フェデレーション ピア (測定された圧力フェデレートのみ) で `near/exceeded` 状態が admission を閉じなくなり、PID を一時停止できなくなります。 |
| Federation admission | インスタンスがディレクトリを共有する場合に有効になります | 共有 `MEMORY_SUPERVISOR_FEDERATION_DIR` を構成します。 WSL ディストリビューション名は自動ですが、他のクローン ゲストには一意の `MEMORY_SUPERVISOR_INSTANCE` 値が必要です | 新しいファンアウトは新しいピア間のみ | 過去 10 秒間の最も悪い有効なスナップショットを使用します。 ハード キャップのプール、RAM 合計の追加、ジョブの移行、リモート構成の変更は決して行われません。 |

<a id="changing-a-supported-cli-memory-budget"></a>
## サポートされている CLI のメモリ バジェットの変更

プロセス ツリーを変更する**各環境内**で次のコマンドを実行します。

```bash
memory-supervisor budget
memory-supervisor budget set 12
memory-supervisor budget off
```

`12` は単なる GiB 構文の例であり、推奨されるサイズではありません (`memory-supervisor hard-cap set <MB>` は MB 精度のエイリアスのままです)。 ベア `budget` レポートには、共有 federation スナップショットを使用して、この環境の理論上の最大値と、ピア環境の明示的な予算を考慮した現在可能な合計値が表示されます。 明示的な予算のみがクレームとしてカウントされ、環境のデフォルトの割り当てはカウントされません。 `set` は、現在可能な合計に対して検証します。過大なリクエストは環境ごとに適合する正確な削減で拒否され、現在可能な合計の 90% 以上のリクエスト、またはマシン全体の明示的な予算の合計を物理的な見積もりの​​ 90% 以上に押し上げるリクエストは確認を求めます (スクリプトの場合は `--yes`)。 `set` は無関係な設定を保存し、そのローカル サービスをリロードします。 `off` は環境を適応専用モードに戻します。

例:

| 望ましい結果 | アクション |
| --- | --- |
| ネイティブ Windows Claude Code および Codex 用の 1 つの共有予算 | PowerShell で `budget set <GiB>` を 1 回実行します |
| WSL セッションの別の予算 | その WSL ディストリビューション内で別の値を実行します |
| ホスト VM とゲスト VM で同じポリシー | 同じコマンドをホスト上で 1 回実行し、ゲスト内で 1 回実行します。 |
| 2 つの VM の異なる予算 | 各 VM 内で異なる値を実行する |
| どこでもデフォルトのスマートな動作 | 以前にオーバーライドがあったすべての環境で `budget off` を実行します |

上限は、サポートされている完全な CLI ルート ツリーを 1 回ずつカウントします。 それはサンプリングされ、ティック間でオーバーシュートする可能性があり、一時停止はすでに常駐しているメモリをすぐには返しません。 バイト単位の割り当て上限には、ネイティブの cgroup、コンテナ、または VM 制限を使用します。

<a id="changing-wsl-or-vm-allocation"></a>
## WSL または VM 割り当ての変更

WSL2 の場合、ホスト側の `%UserProfile%\.wslconfig` は最大共有 WSL VM メモリを設定します。 例：

```ini
[wsl2]
memory=10GB
swap=16GB

[experimental]
autoMemoryReclaim=gradual
```

これは最大値であり、事前割り当てではありません。 これは、WSL VM が完全に停止した後にのみ適用されます。 CLI セッションが終了するため、CLI セッションがアクティブな間は決して `wsl --shutdown` を実行しないでください。 アイドル境界を使用します。 Microsoft の [WSL 構成](https://learn.microsoft.com/windows/wsl/wsl-config) および [`wsl --shutdown`](https://learn.microsoft.com/windows/wsl/basic-commands#shutdown) のドキュメントを参照してください。

Hyper-V、Parallels、VMware、UTM、およびクラウド VM の場合は、通常はゲストが停止している間に、ハイパーバイザーまたはクラウド コントロール プレーンの固定/動的メモリを変更します。 supervisor には一致する番号は必要ありません。ブート後に、ゲスト カーネルが実際に公開しているものを読み取り、再計算します。 ホストとゲストには引き続き個別のインストールが必要で、共有 admission の場合は共有 federation フォルダーが必要です。

<a id="advanced-policy-changes"></a>
## 高度なポリシーの変更

通常のユーザーはこれらを未設定のままにしておく必要があります。 詳細設定は、Unix では `~/.config/memory-supervisor/config.json`、Windows では `$HOME\.config\memory-supervisor\config.json` にあります。

```json
{
  "MEMORY_SUPERVISOR_POLICY_PROFILE": "performance"
}
```

手動編集後、`memory-supervisor update` を実行し、`memory-status` を検査します。 `protect` はより早く動作し、`performance` はより遅く動作し、`balanced` がデフォルトです。 測定された互換性の問題には、きめ細かい `MEMORY_SUPERVISOR_MEM_*`、`MEMORY_SUPERVISOR_PSI_*`、およびプロセス観察オーバーライドが使用できますが、それらの順序は検証され、無効なグループは適応値にフォールバックされます。 傾きまたは生のしきい値は観測値のままです。 共有アクチュエータの不変条件は引き続き一時停止権限を制御します。


<a id="verification-boundary"></a>
## 検証境界

このリポジトリは、GitHub Actions を通じて Linux、Windows、macOS 上で 1 つの共有テスト スイートを実行します。 ネイティブ センサー、プロセス ID、ポリシー決定、hook の動作、インストール ライフサイクル、リリース アーティファクトについて説明します。 制御された Windows および WSL2 ワークロードでは、回復境界付近での停止距離も検証されます。 [テスト マトリックス](../testing/test-matrix.ja.md) および [制動距離の検証](../testing/stopping-distance.ja.md)を参照してください。

ホストされたランナーと決定論的シミュレーションにより、再現可能な製品契約が検証されます。 物理ホスト、ゲスト、コンテナ、または長時間実行されるワークロードのすべての組み合わせを再現するとは主張しません。

<a id="what-is-deliberately-not-possible"></a>
## 意図的に不可能なこと

- 1 つの Windows コマンドでは、WSL、macOS VM、または Linux VM のハード キャップを設定できません。
- WSL インスタンスは Windows PID を一時停止できず、ゲストはホスト PID を一時停止できません。
- Federation は、16 GiB のホスト RAM と 10 GiB の WSL 容量を組み合わせて、架空の 26 GiB プールを作成することはできません。
- supervisor は、電源がオフになっているゲストや、その可視 PID/許可ドメインの外にある CLI を認識しません。
- Apple Silicon 上の macOS VM は Intel Mac テストではありません。 Rosetta は互換性のみをカバーします。
- `MEMORY_SUPERVISOR_CAPACITY_MB` を変更しても、物理メモリの割り当てや再利用は行われません。

インストール パスについては [プラットフォームの展開と federation](platforms.ja.md) を、インスタンスごとの測定されたフットプリントについては [パフォーマンス](performance.ja.md) を参照してください。
