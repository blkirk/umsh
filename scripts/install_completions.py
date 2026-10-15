#!/usr/bin/env python3
"""Install umshctl's shell completion scripts for the current user.

usage: install_completions.py PATH-TO-UMSHCTL

The scripts come from `umshctl completions SHELL`, so they always match
the binary just installed. Each goes where its shell, once set up the
usual way, finds it on its own:

- zsh:  $XDG_DATA_HOME/zsh/site-functions/_umshctl
- bash: $BASH_COMPLETION_USER_DIR/completions/umshctl, which is where
        bash-completion 2 looks ($XDG_DATA_HOME/bash-completion by default)
- fish: $XDG_CONFIG_HOME/fish/completions/umshctl.fish, when fish is
        installed

For the login shell, this then starts that shell the way a terminal
would and checks that it actually picks the completions up. When it
does not, it says what is missing and what to add. Nothing here edits
a shell's startup files: those belong to the user.

Standard library only.
"""

import os
import pathlib
import shutil
import subprocess
import sys

# What a shell's startup files print is theirs; everything after this
# line is the probe's.
MARKER = "__umshctl_completion_probe__"

# A startup file that waits on something should cost a little time,
# not the install.
PROBE_TIMEOUT = 20


def data_home():
    return pathlib.Path(os.environ.get("XDG_DATA_HOME") or pathlib.Path.home() / ".local/share")


def config_home():
    return pathlib.Path(os.environ.get("XDG_CONFIG_HOME") or pathlib.Path.home() / ".config")


def zsh_dir():
    return data_home() / "zsh/site-functions"


def bash_dir():
    # bash-completion reads the first entry of a colon-separated list.
    user_dir = os.environ.get("BASH_COMPLETION_USER_DIR", "").split(":")[0]
    root = pathlib.Path(user_dir) if user_dir else data_home() / "bash-completion"
    return root / "completions"


def fish_dir():
    return config_home() / "fish/completions"


def home_relative(path):
    """`path` as somebody would type it into a startup file."""
    try:
        return "~/" + str(path.relative_to(pathlib.Path.home()))
    except ValueError:
        return str(path)


def generate(umshctl, shell, destination):
    script = subprocess.run(
        [umshctl, "completions", shell], check=True, capture_output=True, text=True
    ).stdout
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(script)
    print(f"installed {shell} completions: {destination}")


def probe(shell_argv):
    """The lines a shell printed after the marker, or None if it could not
    be asked."""
    try:
        result = subprocess.run(
            shell_argv,
            stdin=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            text=True,
            timeout=PROBE_TIMEOUT,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    lines = result.stdout.splitlines()
    if MARKER not in lines:
        return None
    return lines[lines.index(MARKER) + 1 :]


def zsh_steps(directory, report):
    """What still stands between zsh and the completions, from what an
    interactive zsh reported: `compinit` ran, the fpath, and whether
    `umshctl` has a completion bound."""
    if report is None:
        return [
            "Couldn't start an interactive zsh to check its setup. If tab "
            "completion doesn't work in a new shell, make sure ~/.zshrc has:",
            f"    fpath=({home_relative(directory)} $fpath)",
            "    autoload -Uz compinit && compinit",
        ]
    compinit, bound, *fpath = report
    on_fpath = any(
        pathlib.Path(entry).resolve() == directory.resolve() for entry in fpath if entry
    )
    steps = []
    if not on_fpath:
        steps += [
            f"{home_relative(directory)} isn't on zsh's fpath. Add this to ~/.zshrc, "
            "above the line that runs compinit:",
            f"    fpath=({home_relative(directory)} $fpath)",
        ]
    if compinit != "1":
        steps += [
            "zsh's completion system isn't enabled. Add this to ~/.zshrc:",
            "    autoload -Uz compinit && compinit",
        ]
    if not steps and bound != "_umshctl":
        steps += [
            "zsh's completion cache predates the install. Clear it, then open a new shell:",
            "    rm -f ~/.zcompdump*",
        ]
    return steps


def check_zsh(directory):
    report = probe(
        [
            "zsh",
            "-i",
            "-c",
            f"print -r -- {MARKER}; print -r -- ${{+functions[compdef]}}; "
            "print -r -- ${_comps[umshctl]-}; print -rl -- $fpath",
        ]
    )
    return zsh_steps(directory, report)


def bash_steps(script, report):
    """What still stands between bash and the completions, from whether
    an interactive login bash could load a completion for `umshctl`."""
    if report == ["loaded"]:
        return []
    startup = "~/.bash_profile" if sys.platform == "darwin" else "~/.bashrc"
    return [
        "bash doesn't load completions on its own (bash-completion 2 isn't set up). "
        f"Set up bash-completion 2, or add this to {startup}:",
        f"    source {home_relative(script)}",
    ]


def check_bash(script):
    # bash-completion 2.12 names its loader `_comp_load`; earlier 2.x
    # releases, `__load_completion`.
    report = probe(
        [
            "bash",
            "-l",
            "-i",
            "-c",
            f"echo {MARKER}; "
            "if declare -F _comp_load >/dev/null; then _comp_load umshctl; "
            "elif declare -F __load_completion >/dev/null; then __load_completion umshctl; fi; "
            "complete -p umshctl >/dev/null 2>&1 && echo loaded || echo missing",
        ]
    )
    return bash_steps(script, report)


def main(argv):
    if len(argv) != 2:
        sys.exit(__doc__.split("\n\n")[1])
    umshctl = argv[1]

    installed = {}
    if shutil.which("zsh"):
        installed["zsh"] = zsh_dir() / "_umshctl"
    if shutil.which("bash"):
        installed["bash"] = bash_dir() / "umshctl"
    if shutil.which("fish"):
        installed["fish"] = fish_dir() / "umshctl.fish"
    for shell, destination in installed.items():
        generate(umshctl, shell, destination)

    # Only the shell this user logs into is worth starting to check:
    # advice about a shell they never open is noise. fish needs none.
    login = pathlib.Path(os.environ.get("SHELL", "")).name
    if login == "zsh" and "zsh" in installed:
        steps = check_zsh(zsh_dir())
    elif login == "bash" and "bash" in installed:
        steps = check_bash(installed["bash"])
    else:
        steps = []
    if steps:
        print()
        print(f"To finish enabling umshctl tab completion in {login}:")
        for line in steps:
            print(f"  {line}")
    elif login in installed:
        print(f"{login} will complete umshctl in new shells.")


if __name__ == "__main__":
    main(sys.argv)
