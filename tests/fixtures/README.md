# Test fixtures

Generated with ffmpeg (440 Hz sine, tagged title/artist/album/track/genre/date):

```
ffmpeg -f lavfi -i "sine=frequency=440:duration=2:sample_rate=44100" -ac 2 -c:a <codec> \
  -metadata title="Tone <ext>" -metadata artist="Fixture Ensemble" -metadata album="Test Tones" \
  -metadata track=1 -metadata genre=Test -metadata date=2024 tone.<ext>
```

codecs: mp3 = libmp3lame 64k, flac = flac, ogg = libvorbis q0, m4a = aac (LC) 64k,
wav = pcm_s16le (1 s, 22.05 kHz to keep it small).

- `truncated.mp3`: first 4000 bytes of `tone.mp3`.
- `notaudio.flac`: plain text with an audio extension.
