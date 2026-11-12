# Panic Check

A small command-line file integrity monitor written in Rust. It fingerprints every file in a folder with SHA-256, saves the result as a baseline, and later tells you which files were **added**, **modified**, or **deleted**.

> Educational project: a simplified cousin of tools like [AIDE](https://aide.github.io/) and [Tripwire](https://www.tripwire.com/products/tripwire-enterprise/tripwire-file-integrity-manager). Read the [Threat Model](#threat-model) before relying on it.

## Quick start

```bash
cargo install panic-check
panic-check init ./my-folder     # save a baseline
panic-check check ./my-folder    # compare against it
```

Run both commands from the same directory. The baseline is written to, and read from, a file named `baseline.txt` in the **current working directory**.

## Install

Requires the [Rust toolchain](https://rustup.rs/).

**From crates.io**

```bash
cargo install panic-check
```

The binary is installed to `~/.cargo/bin/panic-check` (Windows: `%USERPROFILE%\.cargo\bin\panic-check.exe`).

**From source**

```bash
git clone https://github.com/precious-adeyinka/panic-check.git
cd panic-check
cargo build --release
```

The binary is at `target/release/panic-check`.

## Usage

```text
panic-check <COMMAND> [OPTIONS] <FOLDER>
```

| Command | What it does                                              |
| ------- | --------------------------------------------------------- |
| `init`  | Scans a folder and saves a baseline to `baseline.txt`     |
| `check` | Scans a folder and compares it against the saved baseline |

| Option    | Applies to | What it does                                                         |
| --------- | ---------- | -------------------------------------------------------------------- |
| `--force` | `init`     | Overwrite an existing baseline, or save even when items were skipped |

Folder paths are converted to absolute paths, so `test-dir`, `./test-dir`, and the full path all mean the same folder.

### init

**First run (no baseline yet)**

```bash
panic-check init test-dir
```

```text
Baseline saved: 12 files
```

**A baseline already exists** (exit code 2)

```bash
panic-check init test-dir
```

```text
baseline.txt already exists. Use --force to overwrite.
```

**Overwrite the baseline** (caution: this replaces your reference point)

```bash
panic-check init --force test-dir
```

```text
Baseline saved: 12 files
```

### check

**No changes** (exit code 0)

```bash
panic-check check test-dir
```

```text
No changes
```

**A new file appeared** (exit code 1)

```text
ADDED /home/user/test-dir/new.txt
```

**A file's contents changed** (exit code 1)

```text
MODIFIED /home/user/test-dir/app.conf
```

**A file disappeared** (exit code 1)

```text
DELETED /home/user/test-dir/old.txt
```

**Several changes** give one line per change. Added and modified files appear in path order, followed by deleted files.

```text
MODIFIED /home/user/test-dir/app.conf
ADDED /home/user/test-dir/new.txt
DELETED /home/user/test-dir/old.txt
```

**A renamed file** shows up as one `ADDED` and one `DELETED`. This is expected: the baseline stores paths and hashes, not file identity. Renaming `report.txt` to `summary.txt` gives:

```text
ADDED /home/user/test-dir/summary.txt
DELETED /home/user/test-dir/report.txt
```

**Items that could not be read** (permissions, folders removed mid-scan) are never hidden. They are listed as warnings, and `check` exits with code 1 even if nothing else changed:

```text
WARNING: 2 items skipped:
* /home/user/test-dir/private -> Permission denied (os error 13)
* /home/user/test-dir/locked.db -> Permission denied (os error 13)
```

On Windows, absolute paths may be printed with a `\\?\` prefix. That is just Windows' full-path notation.

## Exit codes

Scripts and CI systems read these to react to results.

| Code | Meaning                                                                                                                                             |
| ---- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| `0`  | `check`: no changes. `init`: baseline saved.                                                                                                        |
| `1`  | `check`: changes were detected, or items were skipped. This is the alert state.                                                                     |
| `2`  | Usage or tool error: bad arguments, folder not found, baseline missing, corrupted, or made from a different folder, or `init` refused to overwrite. |

```bash
panic-check check /srv/app
if [ $? -eq 1 ]; then echo "Integrity alert: files changed"; fi
```

## How it works

1. **Scan:** the folder is walked recursively. Each file is read in 8 KB chunks and fed to SHA-256, so even very large files use almost no memory.
2. **Store:** results are kept in a `BTreeMap`, an ordered map from path to hash. Sorted order makes output and baselines stable between runs.
3. **Compare:** a path missing from the baseline is **added**. A path whose hash differs is **modified**. A baseline path missing from the scan is **deleted**.

### Baseline format

`baseline.txt` is plain text: a header naming the folder it was made from, then one line per file, with the hash first, **two spaces**, then the absolute path.

```text
# folder: /home/user/test-dir
2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824  /home/user/test-dir/hello.txt
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  /home/user/test-dir/empty.txt
```

Update the baseline only after a change you intended, such as a planned upgrade or patch.

### Built-in safeguards

- `init` refuses to overwrite an existing baseline without `--force`.
- `init` refuses to save when items were skipped, unless `--force` is given, so a partial scan cannot silently become your reference.
- `check` refuses to run against a malformed baseline and reports the bad line number.
- `check` refuses to compare when the baseline was made from a different folder.
- Skipped items are always reported, never ignored.

## Threat model

A threat model is an honest statement of what a security tool does and does not defend against.

### Protects against

- Files **added** to the monitored folder after the baseline.
- Files whose **contents were modified**.
- Files that were **deleted**.
- Comparing against a **corrupted baseline**, or one made from a **different folder**: the tool refuses instead of reporting misleading results.
- Silent blind spots: unreadable files and folders are reported, not skipped quietly.

### Does not protect against

- **A tampered baseline.** `baseline.txt` is a plain file. Anyone with write access can regenerate it after changing files. Keep the baseline somewhere an attacker cannot write, such as read-only media or another machine.
- **A tampered tool.** The binary itself is not verified.
- **Metadata changes.** Only file contents are hashed, so permissions and timestamps are not tracked, and neither are folders themselves (a new empty folder goes unnoticed).
- **Changes between scans.** It is a point-in-time check, not real-time monitoring.
- **File identity.** A rename is reported as one `ADDED` and one `DELETED`.
- **Symbolic links.** Symlink behavior is untested. A link pointing back up the folder tree could cause endless scanning.

## Limitations and roadmap

Panic Check is an experimental, learning-focused tool, and mature tools already do this job well. Ideas for where it could go:

- Detect renames by matching identical hashes.
- Store paths relative to the scanned folder, so a baseline survives moving the folder.
- A `--baseline <file>` option instead of the fixed `baseline.txt`.
- Safe symlink handling.
- Track permissions and timestamps.
- Machine-readable (JSON) output.
- Longer term: anomaly detection on change patterns over time.

Ideas and contributions are welcome. Open an issue or get in touch.
