# Windows 可执行文件信任

<p align="center">
  <a href="windows-signing.md">English</a> · <a href="windows-signing.ko.md">한국어</a> · <strong>简体中文</strong> · <a href="windows-signing.ja.md">日本語</a>
</p>

Windows 可执行文件目前正在接受 [SignPath Foundation](https://signpath.org/) 的开源代码签名审核，因此 Smart App Control 在审核完成之前必须在 Windows 11 上保持关闭状态。 当前的准备版本不带有 Authenticode 签名。 安装程序会验证随版本一起发布的 SHA-256 校验和，但完整性验证不会替换 Windows 期望的发布者签名。

<a id="when-this-applies"></a>
## 当这适用时

- PowerShell、Windows 终端和 Windows 本机 Codex App 服务器使用本机 Windows 路径并受 Smart App Control 的约束。
- 如果 Codex App 窗口位于 Windows 上，而其 App Server 和工具在 WSL 内运行，请安装 WSL Supervisor。 没有启动 Windows 可执行文件，因此本指南不适用。
- 组织应用程序控制、Windows 11 S 模式和单独的 SmartScreen 下载信誉检查可能会施加其他限制。 安装程序不会绕过它们。

<a id="current-installation-condition"></a>
## 目前安装情况

Smart App Control 没有每个应用程序的异常。 要使用未签名的本机 Windows 版本，请检查 **Windows 安全 → 应用程序和浏览器控制 → Smart App Control**。

| 窗口状态 | 结果 |
| --- | --- |
| 64 位 Windows 10 | Smart App Control 不可用，因此不需要 SAC 设置。 如果出现 SmartScreen，请验证下载是否来自此存储库的版本。 |
| Smart App Control 已经是 `Off` | 继续进行本机安装。 如果出现单独的 SmartScreen 提示，请验证下载是否来自此存储库的版本。 |
| 当前的 Windows 11 版本显示重新启用控件 | 使用未签名构建时将其设置为`Off`； 之后可以从同一屏幕再次启用它。 |
| Windows 11 不显示重新启用控件 | 关闭它可能需要重置 Windows 或重新安装才能再次打开它，因此请先确认这一点。 |
| 处于 S 模式或阻止组织策略的 Windows 11 | 不支持本机 Windows 路径。 安装程序无法绕过该限制； 如果合适，请使用单独允许的环境，例如 WSL。 |

从 `Win + R` 运行 `winver` 以检查 Windows 版本并构建。 重新启用控件正在 Windows 11 24H2 build 26100.8117 或更高版本以及 25H2 build 26200.8117 或更高版本上推出，因此请在关闭 Smart App Control 之前验证该控件实际上是否可见。 请参阅 Microsoft 的 [Smart App Control 常见问题解答](https://support.microsoft.com/en-US/Windows/Security/Threat-Malware-Protection/smart-app-control-frequently-asked-questions) 和[推出说明](https://support.microsoft.com/en-au/help/5079391) 了解当前标准。

<a id="verifying-a-download"></a>
## 验证下载

一行安装程序下载发布源和可执行文件，然后自动验证其发布的 SHA-256 值。 对于手动下载的可执行文件，请在 PowerShell 中检查其签名状态：

```powershell
Get-AuthenticodeSignature .\memory-supervisor.exe | Format-List Status, StatusMessage, SignerCertificate
```

`NotSigned` 是此准备构建的预期结果。 当发布工件进行代码签名时，安装指南和发行说明将说明该更改，并且此条件将随之更新。
