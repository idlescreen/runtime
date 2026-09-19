# Deployment checklist — fail-OPEN defaults

IdleScreen ships several capabilities as **off by default** for
rollout safety. Operators who want a fully enforced posture must opt
in. This checklist enumerates every off-by-default knob and the env var
that flips it.

> **Presence, not value.** Every gate below checks `var_os().is_some()`
> — setting `IDLE_REQUIRE_MANIFEST_SIGNATURE=0` still *enables* the gate.
> To turn a knob off, unset the variable entirely.

## Mandatory for production deployments

These capabilities exist but are **not enforced** unless the listed env
var is set. Recommended: set ALL of these in your systemd unit, your
CI, and your deployment manifests.

| Capability | Env var to enforce | Default | What changes |
|---|---|---|---|
| Manifest GPG signature verification | `IDLE_REQUIRE_MANIFEST_SIGNATURE=1` | off | Loader refuses any plugin whose `.idleplugin.toml` lacks a valid companion `.sig` against `~/.config/idle/trusted-keys.d/` |
| GPU budget enforcement | `IDLE_GPU_BUDGET=1` | off | `gpu_budget` probes `nvidia-smi` / `intel_gpu_top` / `amdgpu_top` and drops the plugin when usage exceeds `IDLE_GPU_QUOTA_PCT × IDLE_GPU_HARD_MULTIPLIER` |
| CPU budget fail-closed | `IDLE_REQUIRE_CPU_BUDGET=1` | off | When cgroup v2 attach fails, refuse to load the plugin (instead of falling back to in-process measurement) |

## Conditional

Tighten based on your environment.

| Knob | Env var | Default | When to change |
|---|---|---|---|
| IPC read timeout | `IDLE_IPC_READ_TIMEOUT_MS=<ms>` | 500 | Tighten for slow I/O paths; loosen only for debugging |
| Render-loop watchdog | `IDLE_HEARTBEAT_TIMEOUT_MS=<ms>` | 5000 | Tighten for stricter liveness; loosen if false positives |
| Per-plugin tick watchdog | `IDLE_WATCHDOG_TIMEOUT_MS=<ms>` | 250 | Tighten for stricter plugins; loosen for slow savers |
| GPU quota | `IDLE_GPU_QUOTA_PCT=<pct>` | 75 | Lower for shared hosts |
| GPU hard ceiling multiplier | `IDLE_GPU_HARD_MULTIPLIER=<n>` | 2 | Lower for stricter ceiling |
| Trusted keyring location | `IDLE_TRUSTED_KEYS_DIR=<path>` | `~/.config/idle/trusted-keys.d/` | CI / shared-host deployments |
| Sandbox skip (DANGEROUS) | `IDLE_DISABLE_SANDBOX=1` | off | Debug only — never in production |
| Unsigned plugin escape (DANGEROUS) | `IDLE_ALLOW_UNSIGNED_PLUGINS=1` | off | Debug only — never in production |

## Audit

These knobs exist for compatibility with unsigned legacy plugins. They
should **never** be set in production:

- `IDLE_DISABLE_SANDBOX=1` — bypasses Landlock entirely
- `IDLE_ALLOW_UNSIGNED_PLUGINS=1` — bypasses the manifest gate
- `IDLE_PERMIT_NETWORK_PLUGINS=1` — admits plugins declaring `network=true`
- `IDLE_PERMIT_AUDIO_CAPTURE=1` — admits plugins declaring `audio_capture=true`
- `IDLE_PERMIT_AUDIO_OUTPUT=1` — admits plugins declaring `audio_output=true`
- `IDLE_ALLOW_EXPERIMENTAL_PROFILES=1` — admits plugins declaring `experimental` profile
- `IDLE_REQUIRE_CPU_BUDGET=1` not set — CPU budget unenforced on cgroup failure

## Per-saver overrides

Savers that need extra capabilities (network, filesystem, audio) must
declare them in their `.idleplugin.toml`. The host operator decides
whether to admit them; defaults are:

| Capability | Default | Operator override |
|---|---|---|
| `network` | refused under `minimal` profile | `IDLE_PERMIT_NETWORK_PLUGINS=1` |
| `audio_capture` | refused | `IDLE_PERMIT_AUDIO_CAPTURE=1` |
| `audio_output` | refused | `IDLE_PERMIT_AUDIO_OUTPUT=1` |
| `filesystem_read` / `filesystem_write` | sandbox-scoped to manifest paths only | (no opt-in; per-path) |

## systemd unit example

```ini
[Service]
# All recommended fail-OPEN defaults flipped to fail-CLOSED.
Environment="IDLE_REQUIRE_MANIFEST_SIGNATURE=1"
Environment="IDLE_GPU_BUDGET=1"
Environment="IDLE_REQUIRE_CPU_BUDGET=1"
Environment="IDLE_HEARTBEAT_TIMEOUT_MS=3000"
Environment="IDLE_WATCHDOG_TIMEOUT_MS=200"
Environment="IDLE_GPU_QUOTA_PCT=60"
# Sandbox MUST stay on (do NOT set IDLE_DISABLE_SANDBOX).
# Audit escape hatches MUST stay unset.
```

## Verification

After deployment, confirm posture via the install-audit log:

```sh
cat /var/log/idlescreen/install-audit.jsonl | tail -1 | jq .
```

Each entry records the install-time defaults; if `signature.enforce`
is `false`, your deployment is still permissive.

## See also

- `RULES.md` §1.4 default-deny (in this repo)
- `install_audit.sh` — records the audit log shape (packages repo)
- `docs/SIGNING.md` — manifest signing SOP (packages repo)
- `TRUST.md` — installer trust model (packages repo)