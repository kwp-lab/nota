from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import time
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any
from urllib.parse import urlparse

import requests


MODEL = "qwen-audio-3.0-asr-flash-filetrans"
API_ROOT = "https://dashscope.aliyuncs.com/api/v1"
UPLOAD_POLICY_URL = f"{API_ROOT}/uploads"
TRANSCRIPTION_URL = f"{API_ROOT}/services/audio/asr/transcription"
TEMPORARY_OBJECT_LIFETIME = timedelta(hours=48)
STATE_VERSION = 1
SCRIPT_DIRECTORY = Path(__file__).resolve().parent
STATE_PATH = SCRIPT_DIRECTORY / "state.json"
OUTPUT_DIRECTORY = SCRIPT_DIRECTORY / "output"


class ProbeError(RuntimeError):
    pass


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate the DashScope temporary-upload and file-transcription REST flow."
    )
    parser.add_argument("audio", type=Path, help="Local audio file to transcribe")
    parser.add_argument(
        "--speaker-count",
        type=int,
        help="Optional expected speaker count (2-100); omitted means automatic detection",
    )
    parser.add_argument(
        "--language",
        help="Optional language hint such as zh, en, ja, or yue",
    )
    parser.add_argument(
        "--no-diarization",
        action="store_true",
        help="Disable speaker diarization",
    )
    parser.add_argument(
        "--resume",
        action="store_true",
        help="Resume from state.json instead of creating a new upload/task",
    )
    parser.add_argument(
        "--allow-ambiguous-resubmit",
        action="store_true",
        help="Allow a retry when a previous submit may have succeeded without saving its task id",
    )
    parser.add_argument(
        "--poll-initial-seconds",
        type=float,
        default=5.0,
        help="Initial polling interval (default: 5)",
    )
    parser.add_argument(
        "--poll-max-seconds",
        type=float,
        default=30.0,
        help="Maximum polling interval (default: 30)",
    )
    parser.add_argument(
        "--timeout-seconds",
        type=float,
        default=4 * 60 * 60,
        help="Maximum polling duration (default: 14400 / four hours)",
    )
    args = parser.parse_args()
    if args.speaker_count is not None and not 2 <= args.speaker_count <= 100:
        parser.error("--speaker-count must be between 2 and 100")
    if args.speaker_count is not None and args.no_diarization:
        parser.error("--speaker-count cannot be used with --no-diarization")
    if args.poll_initial_seconds <= 0 or args.poll_max_seconds <= 0:
        parser.error("polling intervals must be positive")
    if args.poll_initial_seconds > args.poll_max_seconds:
        parser.error("--poll-initial-seconds cannot exceed --poll-max-seconds")
    if args.timeout_seconds <= 0:
        parser.error("--timeout-seconds must be positive")
    return args


def utc_now() -> datetime:
    return datetime.now(UTC)


def iso_timestamp(value: datetime) -> str:
    return value.isoformat().replace("+00:00", "Z")


def parse_timestamp(value: str) -> datetime:
    return datetime.fromisoformat(value.replace("Z", "+00:00"))


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def audio_fingerprint(path: Path) -> dict[str, Any]:
    resolved = path.resolve(strict=True)
    if not resolved.is_file():
        raise ProbeError("Audio path is not a regular file")
    return {
        "audioPath": str(resolved),
        "audioSizeBytes": resolved.stat().st_size,
        "audioSha256": sha256_file(resolved),
    }


def api_key_from_env_file(path: Path) -> str:
    try:
        lines = path.read_text(encoding="utf-8-sig").splitlines()
    except FileNotFoundError:
        return ""
    except OSError as error:
        raise ProbeError(f"Cannot read local {path.name}: {error}") from error

    for raw_line in lines:
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("export "):
            line = line.removeprefix("export ").lstrip()
        name, separator, value = line.partition("=")
        if separator and name.strip() == "DASHSCOPE_API_KEY":
            value = value.strip()
            if value[:1] in {"\"", "'"}:
                quote = value[0]
                if len(value) < 2 or value[-1] != quote:
                    raise ProbeError("DASHSCOPE_API_KEY in .env has unmatched quotes")
                value = value[1:-1]
            return value.strip()
    return ""


def load_api_key() -> str:
    api_key = os.environ.get("DASHSCOPE_API_KEY", "").strip()
    if api_key:
        return api_key
    return api_key_from_env_file(SCRIPT_DIRECTORY / ".env")


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(
        json.dumps(value, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    temporary.replace(path)


def read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ProbeError(f"Cannot read checkpoint {path.name}: {error}") from error
    if not isinstance(value, dict):
        raise ProbeError(f"Checkpoint {path.name} must contain a JSON object")
    return value


def load_or_create_state(args: argparse.Namespace, fingerprint: dict[str, Any]) -> dict[str, Any]:
    if args.resume:
        if not STATE_PATH.is_file():
            raise ProbeError("--resume was requested but state.json does not exist")
        state = read_json(STATE_PATH)
        if state.get("version") != STATE_VERSION or state.get("model") != MODEL:
            raise ProbeError("state.json belongs to an unsupported probe or model version")
        for key, expected in fingerprint.items():
            if state.get(key) != expected:
                raise ProbeError("The audio file no longer matches the saved checkpoint")
        return state

    if STATE_PATH.exists():
        raise ProbeError(
            "state.json already exists; use --resume or move the existing local probe outputs"
        )
    state = {
        "version": STATE_VERSION,
        "model": MODEL,
        **fingerprint,
        "stage": "new",
        "ossUrl": None,
        "ossExpiresAt": None,
        "taskId": None,
        "createdAt": iso_timestamp(utc_now()),
        "updatedAt": iso_timestamp(utc_now()),
    }
    write_state(state)
    return state


def write_state(state: dict[str, Any]) -> None:
    state["updatedAt"] = iso_timestamp(utc_now())
    write_json(STATE_PATH, state)


def validate_aliyun_https_url(value: str, label: str) -> str:
    parsed = urlparse(value)
    hostname = (parsed.hostname or "").lower()
    if parsed.scheme != "https" or not hostname.endswith(".aliyuncs.com"):
        raise ProbeError(f"{label} did not use an expected Aliyun HTTPS host")
    return value


def request_id(response: requests.Response, body: Any | None = None) -> str | None:
    header_value = response.headers.get("x-request-id")
    if header_value:
        return header_value
    if isinstance(body, dict):
        value = body.get("request_id")
        return str(value) if value else None
    return None


def decode_json_response(response: requests.Response, operation: str) -> dict[str, Any]:
    try:
        body = response.json()
    except requests.exceptions.JSONDecodeError as error:
        if not response.ok:
            write_json(
                OUTPUT_DIRECTORY / "error-response.json",
                {"operation": operation, "status": response.status_code},
            )
        raise ProbeError(
            f"{operation} returned non-JSON HTTP {response.status_code}"
        ) from error
    if not isinstance(body, dict):
        raise ProbeError(f"{operation} returned an unexpected JSON shape")
    if not response.ok:
        write_json(
            OUTPUT_DIRECTORY / "error-response.json",
            {
                "operation": operation,
                "status": response.status_code,
                "requestId": request_id(response, body),
                "body": body,
            },
        )
        code = body.get("code")
        suffix = f" ({code})" if code else ""
        raise ProbeError(f"{operation} failed with HTTP {response.status_code}{suffix}")
    return body


def bearer_headers(api_key: str) -> dict[str, str]:
    return {"Authorization": f"Bearer {api_key}"}


def get_upload_policy(session: requests.Session, api_key: str) -> dict[str, Any]:
    print("Requesting model-bound temporary upload policy...")
    response = session.get(
        UPLOAD_POLICY_URL,
        params={"action": "getPolicy", "model": MODEL},
        headers={**bearer_headers(api_key), "Content-Type": "application/json"},
        timeout=(15, 60),
    )
    body = decode_json_response(response, "upload policy request")
    data = body.get("data")
    if not isinstance(data, dict):
        raise ProbeError("Upload policy response did not contain data")
    required = (
        "policy",
        "signature",
        "upload_dir",
        "upload_host",
        "oss_access_key_id",
        "x_oss_object_acl",
        "x_oss_forbid_overwrite",
    )
    missing = [name for name in required if not data.get(name)]
    if missing:
        raise ProbeError(f"Upload policy response omitted required fields: {', '.join(missing)}")
    validate_aliyun_https_url(str(data["upload_host"]), "Upload policy")
    print(f"Upload policy accepted (request id: {request_id(response, body) or 'unavailable'}).")
    return data


def upload_audio(
    session: requests.Session,
    policy: dict[str, Any],
    audio_path: Path,
) -> tuple[str, datetime]:
    size_bytes = audio_path.stat().st_size
    max_size_value = policy.get("max_file_size_mb")
    if max_size_value is not None:
        try:
            max_size_bytes = int(float(str(max_size_value)) * 1024 * 1024)
        except ValueError as error:
            raise ProbeError("Upload policy returned an invalid max_file_size_mb") from error
        if size_bytes > max_size_bytes:
            raise ProbeError("Audio file exceeds the upload policy size limit")

    object_key = f"{str(policy['upload_dir']).rstrip('/')}/{audio_path.name}"
    upload_host = validate_aliyun_https_url(str(policy["upload_host"]), "Upload host")
    print(f"Uploading audio ({size_bytes} bytes) to temporary private storage...")
    with audio_path.open("rb") as audio:
        fields = [
            ("OSSAccessKeyId", (None, str(policy["oss_access_key_id"]))),
            ("Signature", (None, str(policy["signature"]))),
            ("policy", (None, str(policy["policy"]))),
            ("x-oss-object-acl", (None, str(policy["x_oss_object_acl"]))),
            (
                "x-oss-forbid-overwrite",
                (None, str(policy["x_oss_forbid_overwrite"])),
            ),
            ("key", (None, object_key)),
            ("success_action_status", (None, "200")),
            ("file", (audio_path.name, audio, "audio/ogg")),
        ]
        response = session.post(
            upload_host,
            files=fields,
            timeout=(30, 30 * 60),
        )
    if response.status_code != 200:
        write_json(
            OUTPUT_DIRECTORY / "error-response.json",
            {"operation": "temporary audio upload", "status": response.status_code},
        )
        raise ProbeError(f"Temporary audio upload failed with HTTP {response.status_code}")
    print("Temporary audio upload completed.")
    return f"oss://{object_key}", utc_now() + TEMPORARY_OBJECT_LIFETIME


def temporary_url_is_valid(state: dict[str, Any]) -> bool:
    oss_url = state.get("ossUrl")
    expiry = state.get("ossExpiresAt")
    if not isinstance(oss_url, str) or not oss_url.startswith("oss://"):
        return False
    if not isinstance(expiry, str):
        return False
    return parse_timestamp(expiry) > utc_now() + timedelta(minutes=5)


def submit_task(
    session: requests.Session,
    api_key: str,
    state: dict[str, Any],
    args: argparse.Namespace,
) -> str:
    oss_url = state.get("ossUrl")
    if not temporary_url_is_valid(state) or not isinstance(oss_url, str):
        raise ProbeError("The temporary audio object is missing or too close to expiry")

    parameters: dict[str, Any] = {
        "channel_id": [0],
        "diarization_enabled": not args.no_diarization,
    }
    if args.speaker_count is not None:
        parameters["speaker_count"] = args.speaker_count
    if args.language:
        parameters["language_hints"] = [args.language]
    payload = {
        "model": MODEL,
        "input": {"file_urls": [oss_url]},
        "parameters": parameters,
    }

    state["stage"] = "submitting"
    write_state(state)
    print("Submitting asynchronous transcription task...")
    response = session.post(
        TRANSCRIPTION_URL,
        headers={
            **bearer_headers(api_key),
            "Content-Type": "application/json",
            "X-DashScope-Async": "enable",
            "X-DashScope-OssResourceResolve": "enable",
        },
        json=payload,
        timeout=(15, 120),
    )
    body = decode_json_response(response, "transcription task submission")
    output = body.get("output")
    task_id = output.get("task_id") if isinstance(output, dict) else None
    if not isinstance(task_id, str) or not task_id:
        raise ProbeError("Task submission response did not contain task_id")
    state["taskId"] = task_id
    state["stage"] = "submitted"
    write_state(state)
    print(f"Task accepted (request id: {request_id(response, body) or 'unavailable'}).")
    return task_id


def task_status(body: dict[str, Any]) -> tuple[str, dict[str, Any]]:
    output = body.get("output")
    if not isinstance(output, dict):
        raise ProbeError("Task query response did not contain output")
    status = output.get("task_status")
    if not isinstance(status, str) or not status:
        raise ProbeError("Task query response did not contain task_status")
    return status.upper(), output


def poll_task(
    session: requests.Session,
    api_key: str,
    task_id: str,
    args: argparse.Namespace,
) -> dict[str, Any]:
    interval = args.poll_initial_seconds
    deadline = time.monotonic() + args.timeout_seconds
    previous_status: str | None = None
    while True:
        response = session.get(
            f"{API_ROOT}/tasks/{task_id}",
            headers=bearer_headers(api_key),
            timeout=(15, 60),
        )
        body = decode_json_response(response, "task status query")
        status, output = task_status(body)
        if status != previous_status:
            print(f"Task state: {status}")
            previous_status = status
        if status == "SUCCEEDED":
            write_json(OUTPUT_DIRECTORY / "task-response.json", body)
            return output
        if status in {"FAILED", "CANCELED", "CANCELLED", "UNKNOWN"}:
            write_json(OUTPUT_DIRECTORY / "task-response.json", body)
            code = output.get("code")
            suffix = f" ({code})" if code else ""
            raise ProbeError(f"Task entered terminal state {status}{suffix}")
        if time.monotonic() >= deadline:
            raise ProbeError("Polling timed out; rerun with --resume to continue later")
        time.sleep(interval)
        interval = min(interval * 1.5, args.poll_max_seconds)


def transcription_result_url(output: dict[str, Any]) -> str:
    results = output.get("results")
    if not isinstance(results, list) or not results:
        raise ProbeError("Successful task did not contain any subtask results")
    failures: list[str] = []
    for item in results:
        if not isinstance(item, dict):
            continue
        status = str(item.get("subtask_status", "")).upper()
        url = item.get("transcription_url")
        if status == "SUCCEEDED" and isinstance(url, str) and url:
            return validate_aliyun_https_url(url, "Transcription result")
        if status:
            failures.append(status)
    suffix = f" ({', '.join(failures)})" if failures else ""
    raise ProbeError(f"No successful transcription subtask was available{suffix}")


def download_provider_result(
    session: requests.Session,
    output: dict[str, Any],
) -> dict[str, Any]:
    url = transcription_result_url(output)
    print("Downloading completed provider transcription result...")
    response = session.get(url, timeout=(15, 5 * 60))
    body = decode_json_response(response, "transcription result download")
    write_json(OUTPUT_DIRECTORY / "provider-result.json", body)
    return body


def integer(value: Any, default: int = 0) -> int:
    if isinstance(value, bool):
        return default
    try:
        return int(value)
    except (TypeError, ValueError):
        return default


def speaker_label(value: Any) -> str | None:
    if value is None or isinstance(value, bool):
        return None
    if isinstance(value, int):
        return f"speaker_{max(value, 0)}"
    text = str(value).strip()
    if not text:
        return None
    if text.startswith("speaker_"):
        return text
    try:
        return f"speaker_{max(int(text), 0)}"
    except ValueError:
        return f"speaker_{text}"


def normalize_provider_result(body: dict[str, Any]) -> dict[str, Any]:
    transcripts = body.get("transcripts")
    if not isinstance(transcripts, list):
        raise ProbeError("Provider result did not contain transcripts")

    text_parts: list[str] = []
    segments: list[dict[str, Any]] = []
    language: str | None = None
    for transcript in transcripts:
        if not isinstance(transcript, dict):
            continue
        transcript_text = transcript.get("text", transcript.get("transcript", ""))
        if isinstance(transcript_text, str) and transcript_text.strip():
            text_parts.append(transcript_text.strip())
        if language is None:
            candidate_language = transcript.get("language")
            if isinstance(candidate_language, str) and candidate_language:
                language = candidate_language
        sentences = transcript.get("sentences")
        if not isinstance(sentences, list):
            continue
        for sentence in sentences:
            if not isinstance(sentence, dict):
                continue
            sentence_text = sentence.get("text")
            if not isinstance(sentence_text, str) or not sentence_text.strip():
                continue
            start_ms = max(integer(sentence.get("begin_time")), 0)
            end_ms = max(integer(sentence.get("end_time"), start_ms), start_ms)
            segments.append(
                {
                    "startMs": start_ms,
                    "endMs": end_ms,
                    "text": sentence_text.strip(),
                    "speaker": speaker_label(sentence.get("speaker_id")),
                }
            )

    properties = body.get("properties")
    duration_ms = 0
    if isinstance(properties, dict):
        duration_ms = max(integer(properties.get("original_duration_in_milliseconds")), 0)
    segments.sort(key=lambda item: (item["startMs"], item["endMs"]))
    if segments:
        duration_ms = max(duration_ms, max(item["endMs"] for item in segments))

    text = "\n".join(text_parts).strip()
    if not text and segments:
        text = "".join(item["text"] for item in segments)
    if text and not segments:
        segments.append(
            {
                "startMs": 0,
                "endMs": duration_ms,
                "text": text,
                "speaker": None,
            }
        )
    if not text:
        raise ProbeError("Provider result contained no transcript text")

    return {
        "schemaVersion": "nota-dashscope-probe-1",
        "provider": "dashScope",
        "modelId": MODEL,
        "language": language,
        "durationMs": duration_ms,
        "text": text,
        "segments": segments,
    }


def ensure_audio_supported_for_probe(path: Path) -> None:
    if path.suffix.lower() != ".ogg":
        raise ProbeError("This probe currently expects Nota's original .ogg recording")


def run() -> int:
    args = parse_args()
    ensure_audio_supported_for_probe(args.audio)
    api_key = load_api_key()
    if not api_key:
        raise ProbeError("DASHSCOPE_API_KEY is not set in the environment or local .env")

    fingerprint = audio_fingerprint(args.audio)
    state = load_or_create_state(args, fingerprint)
    audio_path = Path(str(state["audioPath"]))
    session = requests.Session()
    session.headers.update({"User-Agent": "Nota-DashScope-Filetrans-Probe/1"})

    task_id = state.get("taskId")
    if state.get("stage") == "completed":
        normalized_path = OUTPUT_DIRECTORY / "nota-transcript.json"
        if not normalized_path.is_file():
            raise ProbeError("Checkpoint says completed but normalized output is missing")
        normalized = read_json(normalized_path)
        segments = normalized.get("segments")
        speaker_count = len(
            {
                item.get("speaker")
                for item in segments
                if isinstance(item, dict) and item.get("speaker")
            }
        ) if isinstance(segments, list) else 0
        print(
            f"Already completed: {len(segments) if isinstance(segments, list) else 0} segments, "
            f"{speaker_count} anonymous speakers."
        )
        return 0

    if not isinstance(task_id, str) or not task_id:
        stage = state.get("stage")
        if stage == "submitting" and not args.allow_ambiguous_resubmit:
            raise ProbeError(
                "The previous submit outcome is ambiguous. Refusing a possible duplicate billed task; "
                "use --allow-ambiguous-resubmit only after accepting that risk"
            )
        if not temporary_url_is_valid(state):
            if args.resume and state.get("ossUrl"):
                raise ProbeError(
                    "The saved temporary object expired; start a fresh probe after moving state.json"
                )
            policy = get_upload_policy(session, api_key)
            oss_url, expires_at = upload_audio(session, policy, audio_path)
            state["ossUrl"] = oss_url
            state["ossExpiresAt"] = iso_timestamp(expires_at)
            state["stage"] = "uploaded"
            write_state(state)
        task_id = submit_task(session, api_key, state, args)

    output = poll_task(session, api_key, task_id, args)
    provider_result = download_provider_result(session, output)
    normalized = normalize_provider_result(provider_result)
    write_json(OUTPUT_DIRECTORY / "nota-transcript.json", normalized)
    state["stage"] = "completed"
    state["completedAt"] = iso_timestamp(utc_now())
    write_state(state)

    segments = normalized["segments"]
    speakers = {item["speaker"] for item in segments if item.get("speaker")}
    print(
        f"Completed: {len(segments)} normalized segments, "
        f"{len(speakers)} anonymous speakers."
    )
    print(f"Local results: {OUTPUT_DIRECTORY}")
    return 0


def main() -> int:
    try:
        return run()
    except KeyboardInterrupt:
        print("Interrupted locally. Rerun with --resume after task submission.", file=sys.stderr)
        return 130
    except (ProbeError, requests.RequestException) as error:
        print(f"Probe failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
