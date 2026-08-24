import tempfile
import unittest
from pathlib import Path

from transcribe import (
    ProbeError,
    api_key_from_env_file,
    normalize_provider_result,
    speaker_label,
)


class NormalizeProviderResultTests(unittest.TestCase):
    def test_normalizes_sentence_timestamps_and_speakers(self) -> None:
        result = normalize_provider_result(
            {
                "properties": {"original_duration_in_milliseconds": 5000},
                "transcripts": [
                    {
                        "language": "zh",
                        "text": "第一句。第二句。",
                        "sentences": [
                            {
                                "begin_time": 100,
                                "end_time": 2100,
                                "text": "第一句。",
                                "speaker_id": 0,
                            },
                            {
                                "begin_time": 2300,
                                "end_time": 4800,
                                "text": "第二句。",
                                "speaker_id": "1",
                            },
                        ],
                    }
                ],
            }
        )

        self.assertEqual(result["language"], "zh")
        self.assertEqual(result["durationMs"], 5000)
        self.assertEqual(result["segments"][0]["speaker"], "speaker_0")
        self.assertEqual(result["segments"][1]["speaker"], "speaker_1")

    def test_falls_back_to_one_unlabeled_segment_for_plain_text(self) -> None:
        result = normalize_provider_result(
            {
                "properties": {"original_duration_in_milliseconds": 1200},
                "transcripts": [{"transcript": "只有文本", "sentences": []}],
            }
        )

        self.assertEqual(
            result["segments"],
            [{"startMs": 0, "endMs": 1200, "text": "只有文本", "speaker": None}],
        )

    def test_rejects_a_result_without_text(self) -> None:
        with self.assertRaises(ProbeError):
            normalize_provider_result({"transcripts": []})

    def test_preserves_expected_speaker_labels(self) -> None:
        self.assertEqual(speaker_label("speaker_7"), "speaker_7")
        self.assertEqual(speaker_label(None), None)

    def test_reads_quoted_api_key_without_exposing_other_values(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            env_path = Path(directory) / ".env"
            env_path.write_text(
                "IGNORED=value\nDASHSCOPE_API_KEY='test-secret'\n",
                encoding="utf-8",
            )

            self.assertEqual(api_key_from_env_file(env_path), "test-secret")

    def test_missing_env_file_returns_empty_key(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(api_key_from_env_file(Path(directory) / ".env"), "")


if __name__ == "__main__":
    unittest.main()
