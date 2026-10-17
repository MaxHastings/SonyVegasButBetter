"""Reproducible substitute for unavailable user footage; no third-party Python modules.
Three 10-minute recordings (1080p60, 720p29.97, 1080p60) loop a 6-second
moving test pattern. Four 30-second mono/stereo voice-like signals have different
levels, sample rates and silent pauses. These are not actual speech/gameplay.
"""
from pathlib import Path
import subprocess
import json
import time

root = Path(__file__).resolve().parents[1]
ff = root / "runtime" / "ffmpeg.exe"
if not ff.exists():
    ff = Path("ffmpeg")
out = root / "test-output" / "acceptance-media"
out.mkdir(parents=True, exist_ok=True)

def run(*args):
    subprocess.run([str(ff), "-v", "error", "-nostdin", "-y", *map(str,args)], check=True)

start = time.monotonic()
for i, (size, fps, freq) in enumerate([("1920x1080", "60", 330), ("1280x720", "30000/1001", 440), ("1920x1080", "60", 550)], 1):
    seed = out / f"seed-{i}.mp4"
    target = out / f"gameplay-{i}.mp4"
    if target.exists():
        continue
    run("-f", "lavfi", "-i", f"testsrc2=size={size}:rate={fps}:duration=6", "-f", "lavfi", "-i", f"sine=frequency={freq}:sample_rate=48000:duration=6",
        "-c:v", "libx264", "-preset", "ultrafast", "-crf", "30", "-g", "120", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "128k", "-t", "6", seed)
    run("-stream_loop", "99", "-i", seed, "-t", "600", "-c", "copy", "-movflags", "+faststart", target)
    print(f"Created {target.name}", flush=True)
for i, (amp, rate, channels) in enumerate([(0.05,44100,1), (0.15,48000,2), (0.025,32000,1), (0.3,48000,1)], 1):
    target = out / f"microphone-{i}.wav"
    if target.exists():
        continue
    expr = f"{amp}*(sin(2*PI*{160+i*35}*t)+0.25*sin(2*PI*{900+i*100}*t))*(0.5+0.5*sin(2*PI*3*t))*gt(mod(t\,7)\,0.6)"
    run("-f", "lavfi", "-i", f"aevalsrc={expr}:s={rate}:d=30", "-ac", channels, "-c:a", "pcm_s16le", target)
    print(f"Created {target.name}", flush=True)
(out / "dataset.json").write_text(json.dumps({"source_minutes":30,"video_files":3,"audio_files":4,"elapsed_seconds":time.monotonic()-start,
    "limitations":"Six-second moving test patterns repeated to ten minutes; tonal voice-like signals, no genuine speech, difficult gameplay motion or hour-long unique source PTS."},indent=2))
print(f"Dataset ready in {time.monotonic()-start:.1f}s", flush=True)
