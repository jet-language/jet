import unittest
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest.mock import patch

import check_sources


class VideoIdTest(unittest.TestCase):
    def test_url_variants_share_one_id(self):
        expected = "6lXZCOXCRME"
        self.assertEqual(check_sources.video_id(f"https://www.youtube.com/watch?v={expected}&list=PL1"), expected)
        self.assertEqual(check_sources.video_id(f"https://youtu.be/{expected}?t=90"), expected)
        self.assertEqual(check_sources.video_id(f"https://www.youtube.com/shorts/{expected}"), expected)
        self.assertEqual(check_sources.video_id(f"https://www.youtube-nocookie.com/embed/{expected}"), expected)
        self.assertEqual(check_sources.video_id(expected), expected)

    def test_non_youtube_url_is_rejected(self):
        with self.assertRaises(ValueError):
            check_sources.video_id("https://example.com/watch?v=6lXZCOXCRME")

    def test_repeated_batch_id_fails_even_when_rerun_is_allowed(self):
        with TemporaryDirectory() as directory:
            registry = Path(directory) / "prior-art.md"
            registry.write_text("https://www.youtube.com/watch?v=6lXZCOXCRME\n")
            url = "https://www.youtube.com/watch?v=6lXZCOXCRME"
            argv = [
                "check_sources.py",
                "--allow-rerun",
                "6lXZCOXCRME",
                "--registry",
                str(registry),
                url,
                url,
            ]
            with patch("sys.argv", argv):
                with redirect_stdout(StringIO()):
                    self.assertEqual(check_sources.main(), 3)

    def test_tracked_id_requires_allow_rerun(self):
        with TemporaryDirectory() as directory:
            registry = Path(directory) / "prior-art.md"
            registry.write_text("https://www.youtube.com/watch?v=6lXZCOXCRME\n")
            url = "https://youtu.be/6lXZCOXCRME?t=90"
            argv = ["check_sources.py", "--registry", str(registry), url]
            with patch("sys.argv", argv):
                with redirect_stdout(StringIO()):
                    self.assertEqual(check_sources.main(), 3)
            argv[1:1] = ["--allow-rerun", "6lXZCOXCRME"]
            with patch("sys.argv", argv):
                with redirect_stdout(StringIO()):
                    self.assertEqual(check_sources.main(), 0)

    def test_rerun_approval_is_per_video(self):
        with TemporaryDirectory() as directory:
            registry = Path(directory) / "prior-art.md"
            registry.write_text(
                "https://www.youtube.com/watch?v=6lXZCOXCRME\n"
                "https://www.youtube.com/watch?v=2JgEKEd3tw8\n"
            )
            argv = [
                "check_sources.py",
                "--allow-rerun",
                "6lXZCOXCRME",
                "--registry",
                str(registry),
                "https://youtu.be/6lXZCOXCRME",
                "https://youtu.be/2JgEKEd3tw8",
            ]
            with patch("sys.argv", argv):
                with redirect_stdout(StringIO()):
                    self.assertEqual(check_sources.main(), 3)

    def test_verify_tracked_is_read_only_success(self):
        with TemporaryDirectory() as directory:
            registry = Path(directory) / "prior-art.md"
            registry.write_text("https://www.youtube.com/watch?v=6lXZCOXCRME\n")
            argv = [
                "check_sources.py",
                "--verify-tracked",
                "--registry",
                str(registry),
                "https://youtu.be/6lXZCOXCRME",
            ]
            with patch("sys.argv", argv):
                with redirect_stdout(StringIO()):
                    self.assertEqual(check_sources.main(), 0)

    def test_id_inside_longer_token_is_not_tracked(self):
        with TemporaryDirectory() as directory:
            registry = Path(directory) / "prior-art.md"
            registry.write_text("x6lXZCOXCRMEy\n")
            argv = [
                "check_sources.py",
                "--registry",
                str(registry),
                "https://www.youtube.com/watch?v=6lXZCOXCRME",
            ]
            with patch("sys.argv", argv):
                with redirect_stdout(StringIO()):
                    self.assertEqual(check_sources.main(), 0)


if __name__ == "__main__":
    unittest.main()
