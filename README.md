# SonyVegasButBetter (SVBB)

A native, local video editor for cutting gameplay and mixing separate voice
recordings. Rust owns the edit model; FFmpeg and libmpv provide media processing.
No account, telemetry service or network access is needed for editing.

**Personal playtest build, not a public release.** See STATUS.md for the actual
verification results and remaining release gates.

## Run

On Windows, extract the portable package and run `SonyVegasButBetter.exe`.
Keep its `runtime` folder alongside it. No development tools are needed.
For source builds, see BUILDING.md. PLAYTEST.md describes the editing workflow.

## Crop and fit an upload limit

Edit the timeline first, then choose **Render MP4 → Fit under a size limit**.
Enter 10 MB or a custom maximum. Video quality and audio bitrate adjust to the
budget; automatic format adjustment can reduce resolution and frame rate as well.
The export preserves the edited mix volume and timing. CPU two-pass H.264
allocates bits across the edit, and the finished MP4 is measured before acceptance.
An overrun retries at a lower bitrate; failure/cancellation preserves the destination.
MB is decimal (1,000,000 bytes), including audio and container overhead.

Select a video clip and click **Crop** in Preview. Draw a box, drag its interior
to move it, or drag corners to resize. Match project aspect is the default;
Free allows arbitrary rectangles. Preview result shows the framing before Apply.
Enter applies one undoable change; Escape cancels. Reset crop restores the source.
Every clip has its own crop. A split inherits it, then either piece can be changed
independently. Crops save in the project and affect preview, trimming and export.

The size-limit workflow is inspired by Maxwell's [Shorty](https://github.com/MaxHastings/Shorty),
implemented here in the editor's native export pipeline.

## Workflow

Import multiple recordings with Ctrl+I or OS file drop. Append or drag media onto
the video-only V1 and three audio lanes. Split, trim, reposition,
reorder, ripple-delete and undo. Balance source audio on A1 with its track
gain; match microphone clips to a measured loudness target, adjust gain and fades.
Save an SQLite `.svbb` project, reopen it, and render a validated H.264/AAC MP4.

Video is sequential with no overlapping primary clips. Each audio lane allows
gaps but rejects overlaps; use the other lane for overlapping recordings. Source
audio appears on its own linked clip. Move, split and trim either partner to edit
both. Unlink in Clip Properties to edit independently.

## Limits

One video lane, three audio lanes, straight cuts, SDR output. No titles, visual
effects, layering, denoising or plugin hosting. The tested output baseline is
1080p60; preview displays a 960×540 image decoded on demand from originals.
Long files do not need video or PCM copies before editing. Waveforms fill in while
indexing runs in the background. Click a clip once to position the playhead, then
S to split. Drag edges to trim with live source preview; drag top corners to fade.
Auto export tests hardware encoders and falls back to CPU. Render progress shows an
estimated remaining time and the encoder actually used.
Local files only; media with indeterminate duration
or undecodable streams is rejected with an error.

Runtime import checks probe streams and actually decode a frame/block. Container
extensions alone are not promises of codec support. The integration suite covers
H.264, AAC and PCM; further codec/container verification is listed in TESTING.md.

The project does not embed source media. Keep recordings available, or right-click
a missing item in Project Media and select Relink. Projects preserve the first
video stream and first audio stream; alternate source tracks are not exposed.

Application license: GPL-3.0-or-later. See LICENSES.md before redistribution.
