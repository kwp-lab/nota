from __future__ import annotations

import argparse
import sys
import time
import unicodedata
from pathlib import Path
from typing import Any

import requests

from transcribe import (
    API_ROOT,
    MODEL,
    TRANSCRIPTION_URL,
    ProbeError,
    bearer_headers,
    decode_json_response,
    ensure_audio_supported_for_probe,
    get_upload_policy,
    load_api_key,
    normalize_provider_result,
    task_status,
    transcription_result_url,
    upload_audio,
)


EXIT_FAILED = 1
EXIT_INCONCLUSIVE = 2
SUPPORTED_HOTWORD_WEIGHTS = (1, 2, 3, 4, 5, 50)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Manually prove DashScope inline hotwords with an A/B transcription. "
            "This creates two billable asynchronous tasks."
        )
    )
    parser.add_argument("audio", type=Path, help="An Ogg recording containing the target term")
    parser.add_argument(
        "--hotword",
        action="append",
        required=True,
        help="Hotword or short phrase; repeat for multiple entries",
    )
    parser.add_argument(
        "--expected",
        action="append",
        help="Expected transcript spelling; defaults to the --hotword values",
    )
    parser.add_argument(
        "--weight",
        type=int,
        choices=SUPPORTED_HOTWORD_WEIGHTS,
        default=4,
        help="DashScope inline-hotword weight; 50 is a super hotword (default: 4)",
    )
    parser.add_argument("--speaker-count", type=int)
    parser.add_argument("--language", help="Optional language hint such as zh or en")
    parser.add_argument("--poll-seconds", type=float, default=10.0)
    parser.add_argument("--timeout-seconds", type=float, default=4 * 60 * 60)
    parser.add_argument(
        "--confirm-two-paid-tasks",
        action="store_true",
        help="Required acknowledgement that the probe submits two potentially billable tasks",
    )
    args = parser.parse_args()
    if not args.confirm_two_paid_tasks:
        parser.error("--confirm-two-paid-tasks is required")
    if args.speaker_count is not None and not 2 <= args.speaker_count <= 100:
        parser.error("--speaker-count must be between 2 and 100")
    if args.poll_seconds <= 0 or args.timeout_seconds <= 0:
        parser.error("polling interval and timeout must be positive")
    return args


def validate_hotwords(values: list[str]) -> list[str]:
    hotwords: list[str] = []
    seen: set[str] = set()
    for index, raw in enumerate(values, start=1):
        value = raw.strip()
        if not value:
            raise ProbeError(f"Hotword #{index} is empty")
        if len(value) > 100:
            raise ProbeError(f"Hotword #{index} exceeds 100 characters")
        if any(ord(character) > 127 for character in value):
            if len(value) > 15:
                raise ProbeError(f"Non-ASCII hotword #{index} exceeds 15 characters")
        elif len(value.split()) > 7:
            raise ProbeError(f"ASCII hotword #{index} exceeds 7 space-separated parts")
        if value not in seen:
            seen.add(value)
            hotwords.append(value)
    if len(hotwords) > 2_000:
        raise ProbeError("DashScope accepts at most 2,000 inline hotwords")
    return hotwords


def submit_task(
    session: requests.Session,
    api_key: str,
    oss_url: str,
    *,
    hotwords: list[str],
    hotword_weight: int,
    speaker_count: int | None,
    language: str | None,
    label: str,
) -> str:
    parameters = build_parameters(
        hotwords=hotwords,
        hotword_weight=hotword_weight,
        speaker_count=speaker_count,
        language=language,
    )

    print(f"Submitting {label} task...")
    response = session.post(
        TRANSCRIPTION_URL,
        headers={
            **bearer_headers(api_key),
            "Content-Type": "application/json",
            "X-DashScope-Async": "enable",
            "X-DashScope-OssResourceResolve": "enable",
        },
        json={
            "model": MODEL,
            "input": {"file_urls": [oss_url]},
            "parameters": parameters,
        },
        timeout=(15, 120),
    )
    body = decode_json_response(response, f"{label} task submission")
    output = body.get("output")
    task_id = output.get("task_id") if isinstance(output, dict) else None
    if not isinstance(task_id, str) or not task_id:
        raise ProbeError(f"{label} task response did not contain task_id")
    print(f"{label.capitalize()} task accepted (task id: {task_id}).")
    return task_id


def build_parameters(
    *,
    hotwords: list[str],
    hotword_weight: int,
    speaker_count: int | None,
    language: str | None,
) -> dict[str, Any]:
    if hotword_weight not in SUPPORTED_HOTWORD_WEIGHTS:
        raise ProbeError("DashScope hotword weight must be 1-5 or 50")
    parameters: dict[str, Any] = {
        "channel_id": [0],
        "diarization_enabled": True,
    }
    if speaker_count is not None:
        parameters["speaker_count"] = speaker_count
    if language:
        parameters["language_hints"] = [language]
    if hotwords:
        parameters["vocabulary"] = {value: hotword_weight for value in hotwords}
    return parameters


def wait_for_result(
    session: requests.Session,
    api_key: str,
    task_id: str,
    *,
    label: str,
    poll_seconds: float,
    timeout_seconds: float,
) -> str:
    deadline = time.monotonic() + timeout_seconds
    previous_status: str | None = None
    while True:
        response = session.get(
            f"{API_ROOT}/tasks/{task_id}",
            headers=bearer_headers(api_key),
            timeout=(15, 60),
        )
        body = decode_json_response(response, f"{label} task status query")
        status, output = task_status(body)
        if status != previous_status:
            print(f"{label.capitalize()} task state: {status}")
            previous_status = status
        if status == "SUCCEEDED":
            result_url = transcription_result_url(output)
            result_response = session.get(result_url, timeout=(15, 5 * 60))
            provider_result = decode_json_response(
                result_response, f"{label} transcription result download"
            )
            return str(normalize_provider_result(provider_result)["text"])
        if status in {"FAILED", "CANCELED", "CANCELLED", "UNKNOWN"}:
            code = output.get("code")
            suffix = f" ({code})" if code else ""
            raise ProbeError(f"{label} task entered terminal state {status}{suffix}")
        if time.monotonic() >= deadline:
            raise ProbeError(f"Timed out waiting for {label} task {task_id}")
        time.sleep(poll_seconds)


def comparable(value: str) -> str:
    normalized = unicodedata.normalize("NFKC", value).casefold()
    return "".join(character for character in normalized if character.isalnum())


def assert_effect(baseline: str, treatment: str, expected: list[str]) -> int:
    baseline_value = comparable(baseline)
    treatment_value = comparable(treatment)
    missing = 0
    unchanged = 0
    for index, value in enumerate(expected, start=1):
        needle = comparable(value)
        if not needle:
            raise ProbeError(f"Expected spelling #{index} contains no comparable characters")
        baseline_count = baseline_value.count(needle)
        treatment_count = treatment_value.count(needle)
        print(
            f"Target #{index}: baseline matches={baseline_count}, "
            f"hotword matches={treatment_count}."
        )
        if treatment_count == 0:
            missing += 1
        elif treatment_count <= baseline_count:
            unchanged += 1

    if missing:
        print("FAILED: the hotword transcription did not contain every expected spelling.")
        return EXIT_FAILED
    if unchanged:
        print(
            "INCONCLUSIVE: every expected spelling was recognized, but at least one was not "
            "improved over the baseline. Use audio whose rare term is misrecognized without hotwords."
        )
        return EXIT_INCONCLUSIVE
    print("PASSED: every expected spelling improved from baseline to the hotword transcription.")
    return 0


def run() -> int:
    args = parse_args()
    ensure_audio_supported_for_probe(args.audio)
    if not args.audio.is_file():
        raise ProbeError("Audio file does not exist")
    hotwords = validate_hotwords(args.hotword)
    expected = args.expected or hotwords
    api_key = load_api_key()
    if not api_key:
        raise ProbeError("DASHSCOPE_API_KEY is not set in the environment or local .env")

    session = requests.Session()
    session.headers.update({"User-Agent": "Nota-DashScope-Hotword-Effect-Probe/1"})
    policy = get_upload_policy(session, api_key)
    oss_url, _expires_at = upload_audio(session, policy, args.audio.resolve())

    baseline_id = submit_task(
        session,
        api_key,
        oss_url,
        hotwords=[],
        hotword_weight=args.weight,
        speaker_count=args.speaker_count,
        language=args.language,
        label="baseline",
    )
    treatment_id = submit_task(
        session,
        api_key,
        oss_url,
        hotwords=hotwords,
        hotword_weight=args.weight,
        speaker_count=args.speaker_count,
        language=args.language,
        label="hotword",
    )
    baseline = wait_for_result(
        session,
        api_key,
        baseline_id,
        label="baseline",
        poll_seconds=args.poll_seconds,
        timeout_seconds=args.timeout_seconds,
    )
    treatment = wait_for_result(
        session,
        api_key,
        treatment_id,
        label="hotword",
        poll_seconds=args.poll_seconds,
        timeout_seconds=args.timeout_seconds,
    )
    return assert_effect(baseline, treatment, expected)


def main() -> int:
    try:
        return run()
    except KeyboardInterrupt:
        print("Interrupted. Submitted cloud tasks may continue running.", file=sys.stderr)
        return 130
    except (ProbeError, requests.RequestException, OSError) as error:
        print(f"Hotword effect probe failed: {error}", file=sys.stderr)
        return EXIT_FAILED


if __name__ == "__main__":
    raise SystemExit(main())
