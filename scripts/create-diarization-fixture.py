#!/usr/bin/env python3
"""Create fictional three-voice WAVs for check-diarization.ts; never records audio.

Requires macOS's installed Samantha, Daniel and Karen voices, plus ffmpeg.
Only generated fixture files in the explicit output directory are written.
"""
import argparse
import json
from pathlib import Path
import shutil
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--output", type=Path, default=Path(".local/diarization-qa"))
args = parser.parse_args()
if not Path("/usr/bin/say").is_file() or not shutil.which("ffmpeg"):
    parser.error("This optional fixture generator requires macOS say and ffmpeg")
turns = [
    ("Samantha", "The Cedar project review starts now. We are discussing the prototype, the budget, and the testing schedule."),
    ("Daniel", "I will finish the prototype on Monday. The first version includes the dashboard and the export screen."),
    ("Karen", "I will test the dashboard on Tuesday. Please keep the export screen simple so we can verify it quickly."),
    ("Samantha", "The budget remains three thousand dollars. We will keep the scope small and review the results together."),
    ("Daniel", "I agree with that plan. I will share the prototype before lunch on Monday so everyone can try it."),
    ("Karen", "Thank you. I will collect the test results and report the remaining issues during our next review."),
]
args.output.mkdir(parents=True, exist_ok=True, mode=0o700)
for index, (voice, text) in enumerate(turns):
    stem = args.output / f"turn-{index}"
    subprocess.run(["/usr/bin/say", "-v", voice, "-r", "155", "-o", str(stem.with_suffix(".aiff")), text], check=True)
    subprocess.run(["ffmpeg", "-v", "error", "-i", str(stem.with_suffix(".aiff")), "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le", "-af", "apad=pad_dur=0.65", "-y", str(stem.with_suffix(".wav"))], check=True)
(args.output / "concat.txt").write_text("".join(f"file 'turn-{i}.wav'\n" for i in range(len(turns))))
subprocess.run(["ffmpeg", "-v", "error", "-f", "concat", "-safe", "0", "-i", str(args.output / "concat.txt"), "-c", "copy", "-y", str(args.output / "three-speakers.wav")], check=True)
(args.output / "ground-truth.json").write_text(json.dumps({"syntheticOnly": True, "voices": [v for v, _ in turns], "utterances": [t for _, t in turns]}, indent=2))
print(f"Created synthetic three-speakers.wav and single-voice turn-0.wav in {args.output}")
