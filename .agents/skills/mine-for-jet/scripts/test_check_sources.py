import unittest
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest.mock import patch

import check_sources


class StableIdTest(unittest.TestCase):
    def test_youtube_url_variants_share_one_id(self):
        expected = "6lXZCOXCRME"
        self.assertEqual(check_sources.stable_id(f"https://www.youtube.com/watch?v={expected}&list=PL1"), expected)
        self.assertEqual(check_sources.stable_id(f"https://youtu.be/{expected}?t=90"), expected)
        self.assertEqual(check_sources.stable_id(f"https://www.youtube.com/shorts/{expected}"), expected)
        self.assertEqual(check_sources.stable_id(f"https://www.youtube-nocookie.com/embed/{expected}"), expected)
        self.assertEqual(check_sources.stable_id(expected), expected)

    def test_youtube_url_without_video_id_is_rejected(self):
        with self.assertRaises(ValueError):
            check_sources.stable_id("https://www.youtube.com/watch?v=short")

    def test_repo_url_variants_share_one_identity(self):
        expected = "github.com/org/repo"
        self.assertEqual(check_sources.stable_id("https://github.com/org/repo"), expected)
        self.assertEqual(check_sources.stable_id("https://www.github.com/org/repo/"), expected)
        self.assertEqual(check_sources.stable_id("https://github.com/org/repo.git"), expected)
        self.assertEqual(check_sources.stable_id("github.com/org/repo"), expected)

    def test_article_url_drops_query_and_fragment(self):
        self.assertEqual(
            check_sources.stable_id("https://example.com/post?utm_source=x#section"),
            "example.com/post",
        )

    def test_bare_word_is_rejected(self):
        with self.assertRaises(ValueError):
            check_sources.stable_id("not-a-url")


class RegistryCheckTest(unittest.TestCase):
    def run_main(self, argv):
        with patch("sys.argv", ["check_sources.py", *argv]):
            with redirect_stdout(StringIO()):
                return check_sources.main()

    def test_repeated_batch_id_fails_even_when_rerun_is_allowed(self):
        with TemporaryDirectory() as directory:
            registry = Path(directory) / "prior-art.md"
            registry.write_text("https://www.youtube.com/watch?v=6lXZCOXCRME\n")
            url = "https://www.youtube.com/watch?v=6lXZCOXCRME"
            argv = ["--allow-rerun", "6lXZCOXCRME", "--registry", str(registry), url, url]
            self.assertEqual(self.run_main(argv), 3)

    def test_tracked_id_requires_allow_rerun(self):
        with TemporaryDirectory() as directory:
            registry = Path(directory) / "prior-art.md"
            registry.write_text("https://www.youtube.com/watch?v=6lXZCOXCRME\n")
            url = "https://youtu.be/6lXZCOXCRME?t=90"
            argv = ["--registry", str(registry), url]
            self.assertEqual(self.run_main(argv), 3)
            self.assertEqual(self.run_main(["--allow-rerun", "6lXZCOXCRME", *argv]), 0)

    def test_rerun_approval_is_per_source(self):
        with TemporaryDirectory() as directory:
            registry = Path(directory) / "prior-art.md"
            registry.write_text(
                "https://www.youtube.com/watch?v=6lXZCOXCRME\n"
                "https://www.youtube.com/watch?v=2JgEKEd3tw8\n"
            )
            argv = [
                "--allow-rerun",
                "6lXZCOXCRME",
                "--registry",
                str(registry),
                "https://youtu.be/6lXZCOXCRME",
                "https://youtu.be/2JgEKEd3tw8",
            ]
            self.assertEqual(self.run_main(argv), 3)

    def test_verify_tracked_is_read_only_success(self):
        with TemporaryDirectory() as directory:
            registry = Path(directory) / "prior-art.md"
            registry.write_text("https://www.youtube.com/watch?v=6lXZCOXCRME\n")
            argv = ["--verify-tracked", "--registry", str(registry), "https://youtu.be/6lXZCOXCRME"]
            self.assertEqual(self.run_main(argv), 0)

    def test_id_inside_longer_token_is_not_tracked(self):
        with TemporaryDirectory() as directory:
            registry = Path(directory) / "prior-art.md"
            registry.write_text("x6lXZCOXCRMEy\n")
            argv = ["--registry", str(registry), "https://www.youtube.com/watch?v=6lXZCOXCRME"]
            self.assertEqual(self.run_main(argv), 0)

    def test_tracked_repo_matches_registry_url_forms(self):
        with TemporaryDirectory() as directory:
            registry = Path(directory) / "prior-art.md"
            registry.write_text("Mined: https://www.github.com/org/repo.git (2026-08-01)\n")
            argv = ["--registry", str(registry), "https://github.com/org/repo"]
            self.assertEqual(self.run_main(argv), 3)
            self.assertEqual(self.run_main(["--allow-rerun", "github.com/org/repo", *argv]), 0)

    def test_repo_prefix_of_longer_path_is_not_tracked(self):
        with TemporaryDirectory() as directory:
            registry = Path(directory) / "prior-art.md"
            registry.write_text(
                "https://github.com/org/repository\n"
                "https://github.com/org/repo/issues/5\n"
                "https://notgithub.com/org/repo\n"
            )
            argv = ["--registry", str(registry), "https://github.com/org/repo"]
            self.assertEqual(self.run_main(argv), 0)

    def test_tracked_article_matches_bare_registry_line(self):
        with TemporaryDirectory() as directory:
            registry = Path(directory) / "prior-art.md"
            registry.write_text("example.com/post — mined 2026-08-01\n")
            argv = ["--registry", str(registry), "https://example.com/post?utm_source=x"]
            self.assertEqual(self.run_main(argv), 3)


if __name__ == "__main__":
    unittest.main()
