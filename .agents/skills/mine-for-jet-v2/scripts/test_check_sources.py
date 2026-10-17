import json
import unittest
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest.mock import patch

import check_sources


class SourceGuardTest(unittest.TestCase):
    def run_guard(self, registry, *urls, options=()):
        output = StringIO()
        with patch("sys.argv", ["check_sources.py", "--registry", str(registry), *options, *urls]):
            with redirect_stdout(output):
                code = check_sources.main()
        return code, json.loads(output.getvalue())

    def test_youtube_and_repository_historical_ids_are_unchanged(self):
        for value in ["https://www.youtube.com/watch?v=6lXZCOXCRME&list=PL1",
                      "https://youtu.be/6lXZCOXCRME?t=90", "https://youtube.com/shorts/6lXZCOXCRME",
                      "https://youtube-nocookie.com/embed/6lXZCOXCRME", "6lXZCOXCRME"]:
            self.assertEqual(check_sources.stable_id(value), "6lXZCOXCRME")
        for value in ["https://github.com/org/repo", "https://www.github.com/org/repo.git/", "github.com/org/repo"]:
            self.assertEqual(check_sources.stable_id(value), "github.com/org/repo")
        self.assertEqual(check_sources.stable_id("https://example.org/p?edition=2#code"), "example.org/p")

    def test_content_selectors_survive_but_known_tracking_does_not(self):
        self.assertEqual(check_sources.content_id("https://example.org/p?utm_source=x&edition=2&edition=3#code"),
                         "example.org/p?edition=2&edition=3#code")
        self.assertNotEqual(check_sources.content_id("https://example.org/p?edition=2"),
                            check_sources.content_id("https://example.org/p?edition=3"))
        self.assertNotEqual(check_sources.content_id("https://example.org/p#code"), check_sources.content_id("https://example.org/p#results"))

    def test_invalid_video_or_playlist_does_not_expand_scope(self):
        for value in ["https://youtube.com/watch?v=short", "https://youtube.com/playlist?list=PL1", "not-a-url"]:
            with self.assertRaises(ValueError):
                check_sources.stable_id(value)

    def test_batch_duplicate_and_rerun_exit_conventions(self):
        with TemporaryDirectory() as root:
            registry = Path(root) / "prior-art.md"
            registry.write_text("https://youtu.be/6lXZCOXCRME\n")
            url = "https://youtube.com/watch?v=6lXZCOXCRME"
            self.assertEqual(self.run_guard(registry, url)[0], 3)
            self.assertEqual(self.run_guard(registry, url, options=("--allow-rerun", "6lXZCOXCRME"))[0], 0)
            self.assertEqual(self.run_guard(registry, url, url, options=("--allow-rerun", "6lXZCOXCRME"))[0], 3)
            self.assertEqual(self.run_guard(registry, url, options=("--verify-tracked",))[0], 0)
            self.assertEqual(self.run_guard(registry, "https://example.org/new", options=("--verify-tracked",))[0], 4)

    def test_query_distinct_resources_are_not_batch_duplicates_and_history_is_visible(self):
        with TemporaryDirectory() as root:
            registry = Path(root) / "prior-art.md"
            registry.write_text("example.org/p\n")
            code, rows = self.run_guard(registry, "https://example.org/p?edition=2", "https://example.org/p?edition=3")
            self.assertEqual(code, 0)
            self.assertEqual([row["source_id"] for row in rows], ["example.org/p", "example.org/p"])
            self.assertEqual([row["status"] for row in rows], ["new", "new"])
            self.assertEqual(rows[0]["historical_selector_unknown_lines"], [1])

    def test_registry_url_with_tracking_normalizes_but_longer_path_is_not_same_source(self):
        with TemporaryDirectory() as root:
            registry = Path(root) / "prior-art.md"
            registry.write_text("https://example.org/p?utm_source=x\nhttps://github.com/org/repo/issues/5\n")
            self.assertEqual(self.run_guard(registry, "https://example.org/p")[0], 3)
            self.assertEqual(self.run_guard(registry, "https://github.com/org/repo")[0], 0)

    def test_digest_report_links_absent_from_registry_are_candidates_not_proof_of_body_review(self):
        with TemporaryDirectory() as root:
            registry = Path(root) / "prior-art.md"
            registry.write_text("")
            evidence = Path(root) / "digest.json"
            evidence.write_text(json.dumps({"schema": "jet-state:v1", "research": [{
                "path": "Docs/research/example.md", "source_urls": ["https://example.org/p?edition=2"],
                "coverage": "text indexed; body not technically reviewed", "state": "dated-research-not-current-law"
            }]}))
            code, rows = self.run_guard(registry, "https://example.org/p?edition=2", options=("--evidence-index", str(evidence)))
            self.assertEqual(code, 3)
            self.assertEqual(rows[0]["report_candidates"][0]["report"], "Docs/research/example.md")
            self.assertIn("unknown", rows[0]["coverage"])
            self.assertEqual(self.run_guard(registry, "https://example.org/p?edition=3", options=("--evidence-index", str(evidence)))[0], 0)


if __name__ == "__main__":
    unittest.main()
