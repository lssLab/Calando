# Security and data/control boundaries

<p align="center">
  <strong>English</strong> · <a href="security.ko.md">한국어</a> · <a href="security.zh-CN.md">简体中文</a> · <a href="security.ja.md">日本語</a>
</p>

This document lists every category of information Calando reads,
stores locally, and shares across environments on one computer. It also
describes the network access used by updates and optional Telegram notices and the exact
control limits. The table is the product's complete data and control boundary;
information and programs outside it are not handled.

| Area | What it reads or does | Explicit storage, sharing, and control boundary |
| --- | --- | --- |
| System memory | Reads operating-system totals for physical, available, and committed memory; rate of decline; memory pressure; reclaim, swap, and paging activity; and an applicable container memory limit. | It does not read memory contents or change RAM, swap, container limits, or virtual-machine memory settings. |
| Running processes | Reads the executable and command-line metadata, PID, parent relationship, start identity, terminal, memory use, and growth rate visible to the current permission level to identify Claude Code, Codex, and their child work. | It does not read process-memory contents or files opened by a program, and it does not persist complete raw process command lines in state. For unrelated programs such as Chrome or an IDE, it reads only the operating-system process metadata needed for attribution and does not control them. |
| Claude Code and Codex hooks | The hook program receives each event JSON transiently, then extracts only provider, hook event, session, conversation thread, agent, turn, task, and tool identifiers and types; App Server routing data; working directory; at most the first 2,048 characters of a command to be started; and restriction receipts. | Even if an event contains a prompt, conversation or model text, or file content for an edit, those fields are not used for control or placed in local state, federation, or Telegram notices. It does not query Claude or ChatGPT credential stores or tokens. A secret entered directly on a command line can appear in the extracted local command prefix, so secrets should not be placed on command lines. |
| Local storage | Stores configuration, memory decisions, process and agent state, action and recovery history, hook health, and notification settings under the user's profile. | It does not write into project files. Raw working directories and command prefixes stay local and are not sent in Telegram notices. |
| Same-machine federation | Each environment publishes current memory, pressure, and action state; process, terminal, and agent identifiers; App hook paths and health; and incident and recovery state to the same local shared folder. Peers use only state refreshed within ten seconds to align new-work admission. A peer is accepted only if its `instance` is a plain identifier (letters, digits, `.`, `-`, `_`, up to 128 characters) equal to its file name. | Each monitor controls only processes in its own environment. Network shares and state from another physical computer or cloud server are not used for federation. Raw working directories and command prefixes are not included in federated state. Peer text never reaches agent context. |
| Network and notifications | Normal monitoring makes no external request. Installation, update, and the daily update check contact GitHub. An enabled Telegram route sends sanitized memory state, action, and reason; `calando telegram` contacts the Telegram Bot API. | This project has no collection server or usage telemetry. Notification credentials stay in local configuration and are masked in status output. Prompts, conversation text, raw commands, and project files are not sent. |
| New-work control | Before launch, hooks can allow or delay a new subagent, tool, build, test, or other high-memory command, and progressively narrow only the selected agent's future work. | It does not cancel editing, responses, or result delivery already in progress, and it does not type commands into the terminal. |
| Process control | Only as a last resort, pauses one local work process revalidated as belonging to Claude Code or Codex, then resumes processes one at a time after stable recovery. It rechecks the PID and start identity immediately before acting. | It never terminates or force-terminates a process and does not pause another program or a PID in another environment. |
| Lead-agent protection | A lead pauses only in the rare case where danger remains after staged new-work delays and subagent and tool controls, and sustained growth from that same lead, its exact process, and its terminal have all been confirmed. | A lead pause is not an ordinary memory-pressure action; it is the final protection stage. The lead is not paused if its exact terminal cannot receive the notice. |
| Terminal and lead notices | Writes the reason for a restriction and recovery state to a verified terminal and delivers the same decision to the lead through the next hook. | It does not read terminal input, inject a command, or change terminal settings or input modes. |
| Installation, update, and removal | Installs, refreshes, or removes only its user-scoped executable or source, user background service and autostart entry, owned hook and skill connections, and configuration and state files. | It does not modify project files, the kernel, drivers, the firewall, or operating-system memory and swap settings. Removal starts only when the user explicitly runs `uninstall`. Automatic updates install only GitHub releases. |
| User-selected settings | There is no fixed memory cap by default. `cap`, `on` and `off`, `update auto`, `lock`, and Telegram settings change only when the user (or a same-user agent) runs those commands. | It does not enable an optional cap automatically. |
| Safeguards | A missing or stale hook decision is discarded, work passes through, and protection is reported unavailable. The PID and start identity are rechecked before process control. A lead is not paused unless its exact terminal can receive the notice; if the action record cannot be saved, it is not kept paused. | It does not leave stale decisions blocking work or retain an unrecorded pause. Automatic physical action never exceeds a reversible pause of a verified Claude Code or Codex work process. |

**Calando inspects and controls only what is listed in this table.**
It does not open project files or process memory directly. Prompt, conversation,
model-response, and file-content fields that may arrive inside a hook event are
not extracted or used for control and are not persisted to local state, shared
through federation, or sent in external notifications. Browser and IDE internal
data and Claude or ChatGPT credentials are also outside its boundary.

## Isolation

- Hooks run with the user's rights. Calando never does for a caller anything that caller could not
  already do. Agents running as the same user may configure it; this is by design, so an agent can
  help the user.
- `calando lock on` is the only way to stop same-user agents from turning Calando off. It writes a
  policy file only an administrator can change (`/etc/calando/lock`, Windows
  `%ProgramData%\Calando\lock`); while it exists, `off` and `uninstall` refuse. It is not a sandbox:
  a same-user process without a sandbox can still stop the service or forge federation files. Forged
  federation state can only hold work, that is, deny new work, which the same process could also
  cause by using memory.
- On WSL, `/mnt/c` is readable and writable by every Linux user of the distribution, so WSL Linux users
  are not an isolation boundary.
- Releases carry SHA-256 checksums and GitHub build provenance attestations.
