These are synthetic six-second test-pattern H.264/AAC clips generated locally
with FFmpeg 8.1; they contain no source-site media. Each playlist has three
two-second segments. `aes-ts` uses AES-128 CBC with the public test key in
`key.bin`; `plain-fmp4` uses EXT-X-MAP. They exercise the actual Rust downloader
and the repository-local FFmpeg remuxer over a loopback HTTP test server.

Generation used `testsrc2=size=160x284:rate=25`, `sine=frequency=880:sample_rate=44100`,
`-t 6 -c:v libx264 -preset ultrafast -g 50 -keyint_min 50 -sc_threshold 0`,
`-c:a aac -f hls -hls_time 2 -hls_playlist_type vod`, with `-hls_key_info_file`
for TS and `-hls_segment_type fmp4 -hls_fmp4_init_filename plain-fmp4-init.mp4`
for fMP4. Test key bytes are `key.bin` and explicit IV is zero. The generator
is a development tool; the bundled remux-only FFmpeg has no x264 dependency.
