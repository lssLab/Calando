# Federation across environments on one machine

<p align="center">
  <strong>English</strong> · <a href="federation-topology.ko.md">한국어</a>
</p>

Memory Supervisor runs once in each operating-system kernel that can see and control its own
processes. Federation lets those supervisors coordinate when several kernels compete for the same
physical memory.

## The deciding rule

Two environments federate only when both conditions are true:

1. their memory can grow against the same physical RAM; and
2. they can meet through a host-local shared directory that proves they are on the same machine.

Federation shares the current new-work decision. It does not pool memory, move work, or give one
kernel authority over another kernel's process IDs.

## Topology matrix

| Environment | Shares host RAM dynamically? | Federation behavior |
| --- | --- | --- |
| Native Linux, macOS, or Windows | No second kernel | Local supervisor only |
| WSL2 and its Windows host | Yes | Meet through the host filesystem and share admission |
| Several WSL2 distributions | Yes | Meet through the same host filesystem |
| Host and ordinary containers | Yes | Federate when the same host-local directory is mounted |
| Dynamic-memory VM with a balloon device | Yes | Federate when a host-local shared folder is available |
| Fixed-memory VM | No; memory is partitioned | Guest stays independent |
| Cloud VM or another physical computer | No manageable shared RAM | Each machine stays independent |

A second computer never becomes a federation peer merely because both computers can access the
same network location. A network share is not proof of shared physical memory.

## Detection and rendezvous

The topology adapter separates three questions:

```text
OS adapter          -> how this kernel measures memory and controls a verified local PID
AI adapter          -> how Claude Code or Codex exposes sessions, agents, and tools
topology adapter    -> which co-resident kernels share RAM and where they exchange state
```

WSL2 uses the mounted Windows host filesystem. Containers use an explicitly mounted host-local
directory. A dynamic VM uses an available hypervisor shared folder; otherwise it stays local-only.
Linux guests are classified as dynamic when a supported memory-balloon device is present and fixed
when it is absent.

`memory-status --all` shows the detected environments and the decision contributed by each one.
Only state refreshed within the freshness window participates.

## What crosses the boundary

Federated state contains only the information needed to coordinate protection:

- memory capacity, headroom, pressure, and rate;
- current admission and recovery state;
- environment, process, terminal, and logical-agent identifiers needed for diagnosis;
- hook connection health and pending incident state.

It does not include prompts, conversations, model responses, project-file contents, full command
lines, credentials, or notification secrets. Each supervisor can pause or resume only a process
that it revalidates inside its own kernel.

## Host-local safety check

The rendezvous directory must be local to the physical host. Linux rejects network filesystem
types while allowing host-local mounts such as WSL2's host filesystem. macOS requires a local
filesystem flag. Windows rejects UNC paths and mapped remote drives.

If the topology or directory is ambiguous, the supervisor isolates the environment instead of
trusting a possible remote peer. The result can be missed coordination, but not process control on
another machine.

## Multi-terminal behavior

Several terminals in one kernel are already observed by the same local supervisor. They do not
create additional daemons or duplicate memory totals. If Windows, WSL2, a container, or a dynamic
guest adds another kernel on the same machine, each kernel runs its own supervisor and federation
aligns only the new-work decision.

The worst fresh admission state governs the shared resource. Process containment remains local and
selective: a supervisor never signals a peer environment's PID.

See [platform deployment](platforms.md) for setup and the
[test matrix](../testing/test-matrix.md) for verified coverage.
