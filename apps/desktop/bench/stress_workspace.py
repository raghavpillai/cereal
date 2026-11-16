#!/usr/bin/env python3
"""Write a large synthetic workspace for benchmarking: stress_workspace.py <data-dir>

500 notes (50 of them in Trash). Note 1, which the tour opens, has an
80-line enhanced summary, a 4 KB body and a 1,500-utterance transcript; 20 more notes
carry hour-long transcripts. Entirely fictional text; never point it at a real data directory.
"""
import json, os, random, sys

def main(directory):
    random.seed(7)
    words = ("review plan launch design search calendar onboarding prototype customer "
             "feedback metrics roadmap budget hiring release notes summary action owner").split()
    sentence = lambda n: " ".join(random.choice(words) for _ in range(n)).capitalize() + "."
    def transcript(turns):
        out = []
        for i in range(turns):
            out.append(f"{'Maya' if i % 2 == 0 else 'Theo'}  {i * 7 // 60:02d}:{i * 7 % 60:02d}\n{sentence(12)}")
        return "\n\n".join(out)
    def enhanced(sections):
        out = []
        for s in range(sections):
            out.append(f"## {sentence(3)[:-1]}\n")
            out.extend(f"- {sentence(9)}" for _ in range(7))
            out.append("")
        return "\n".join(out)
    notes = []
    for i in range(1, 501):
        long = i == 1 or 2 <= i <= 21
        notes.append({
            "id": i,
            "title": f"{sentence(3)[:-1]} {i}",
            "body": (sentence(10) + "\n") * (60 if i == 1 else 3),
            "enhanced": enhanced(10 if i == 1 else 2),
            "transcript": transcript(1500 if i == 1 else 500 if long else 6),
            "date": f"2026-{11 - (i // 60) % 10:02d}-{28 - i % 27:02d}",
            "time": f"{9 + i % 8}:{(i * 7) % 60:02d} {'AM' if i % 2 else 'PM'}",
            "duration": "32 min",
            "favorite": i % 11 == 0,
            "trashed": i > 450,
            "transcriptRevision": 1,
            "generatedRevision": 1,
        })
    os.makedirs(directory, exist_ok=True)
    path = os.path.join(directory, "workspace.json")
    with open(path, "w") as file:
        json.dump({"notes": notes,
                   "settings": {"dark_mode": False, "auto_enhance": False}}, file)
    print(f"{path}: {len(notes)} notes, {os.path.getsize(path) // 1024} KB")

if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])
