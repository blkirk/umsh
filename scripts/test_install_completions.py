"""What the completion installer tells somebody whose shell is not set up."""

import pathlib
import unittest

from install_completions import bash_steps, zsh_steps

DIRECTORY = pathlib.Path("/home/somebody/.local/share/zsh/site-functions")
SCRIPT = pathlib.Path("/home/somebody/.local/share/bash-completion/completions/umshctl")


class ZshStepsTests(unittest.TestCase):
    def test_a_working_setup_needs_nothing(self):
        report = ["1", "_umshctl", "/usr/share/zsh/functions", str(DIRECTORY)]
        self.assertEqual(zsh_steps(DIRECTORY, report), [])

    def test_a_missing_fpath_entry_and_compinit_are_both_named(self):
        steps = "\n".join(zsh_steps(DIRECTORY, ["0", "", "/usr/share/zsh/functions"]))
        self.assertIn(f"fpath=({DIRECTORY} $fpath)", steps)
        self.assertIn("autoload -Uz compinit && compinit", steps)

    def test_a_stale_cache_is_named_only_once_the_rest_is_right(self):
        steps = "\n".join(zsh_steps(DIRECTORY, ["1", "", str(DIRECTORY)]))
        self.assertIn("rm -f ~/.zcompdump*", steps)
        steps = "\n".join(zsh_steps(DIRECTORY, ["1", "", "/usr/share/zsh/functions"]))
        self.assertNotIn("zcompdump", steps)

    def test_a_shell_that_could_not_be_asked_gets_the_whole_recipe(self):
        steps = "\n".join(zsh_steps(DIRECTORY, None))
        self.assertIn("fpath=", steps)
        self.assertIn("compinit", steps)


class BashStepsTests(unittest.TestCase):
    def test_a_loaded_completion_needs_nothing(self):
        self.assertEqual(bash_steps(SCRIPT, ["loaded"]), [])

    def test_otherwise_the_script_can_be_sourced(self):
        for report in (["missing"], None):
            self.assertIn(f"source {SCRIPT}", "\n".join(bash_steps(SCRIPT, report)))


if __name__ == "__main__":
    unittest.main()
