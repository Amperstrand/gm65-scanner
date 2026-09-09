"""HIL fixtures: bench flock first, labgrid place when the coordinator is up.

Markers:
    hardware       — needs a bench board
    flash_mutation — flashes firmware (opt-in via -m flash_mutation selection)

Bench exclusivity follows the Amperstrand playbook pattern 12: the shared
flock (/tmp/amperstrand-bench.lock via tollgate_lab) FIRST, then the labgrid
place — never the reverse. The place doubles as a state record (tags).
"""

import json
import subprocess
import time
from pathlib import Path

import pytest

from tollgate_lab import BenchLockHeldError, acquire_bench_lock

import rig

RESULTS_DIR = Path(__file__).parent / "results"
LEDGER_PATH = RESULTS_DIR / "history.jsonl"
PLACE = "gm65-qr-loopback"
COORDINATOR = "192.168.13.221:20408"

LG_ACQUIRED_KEY = pytest.StashKey[bool]()


def pytest_configure(config):
    for marker in (
        "hardware: requires a bench board",
        "flash_mutation: flashes firmware onto registry boards — opt-in only",
    ):
        config.addinivalue_line("markers", marker)


def _lg(*args, timeout_s=15):
    return subprocess.run(
        ["labgrid-client", "-x", COORDINATOR, "-p", PLACE, *args],
        capture_output=True, text=True, timeout=timeout_s,
    )


def _lg_coordinator_up() -> bool:
    try:
        return _lg("who", timeout_s=10).returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        return False


def _lg_acquire_or_exit():
    acquired = _lg("acquire")
    if acquired.returncode != 0:
        pytest.exit(
            f"labgrid place {PLACE} is acquired by another session "
            f"({acquired.stderr.strip() or 'see labgrid-client who'})",
            returncode=3,
        )


def pytest_sessionstart(session):
    if _lg_coordinator_up():
        _lg_acquire_or_exit()
        session.stash[LG_ACQUIRED_KEY] = True


@pytest.fixture(scope="session")
def rig_lock(request):
    try:
        bench_lock = acquire_bench_lock(
            "amperstrand-bench", project="gm65-hil",
            cwd=str(Path(__file__).resolve().parents[2]),
        )
    except BenchLockHeldError as exc:
        pytest.exit(f"bench flock held: {exc}", returncode=3)

    yield "labgrid-place" if request.session.stash.get(LG_ACQUIRED_KEY, False) else "flock"
    if request.session.stash.get(LG_ACQUIRED_KEY, False):
        _lg("release")
    bench_lock.release()


@pytest.fixture(scope="session")
def artifacts_dir():
    run_dir = RESULTS_DIR / time.strftime("run-%Y%m%d-%H%M%S")
    run_dir.mkdir(parents=True, exist_ok=True)
    return run_dir


def pytest_sessionfinish(session, exitstatus):
    if session.stash.get(LG_ACQUIRED_KEY, False):
        _lg("release")
    RESULTS_DIR.mkdir(exist_ok=True)
    record = {
        "ts": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
        "exit": exitstatus,
    }
    try:
        with open(LEDGER_PATH, "a") as f:
            f.write(json.dumps(record) + "\n")
    except OSError:
        pass
