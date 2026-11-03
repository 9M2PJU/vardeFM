# Changelog

All notable changes to VardeFM. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Until 1.0 a minor version bump (0.x) means the on-air protocol changed: both stations must
run the same minor version.

## [0.3.0] - 2026-11-03

**Not on-air compatible with 0.2.x.** The LDPC codes, the per-level transmit drive and the
rate-adaptation thresholds all changed. Both stations must upgrade.

### Changed
- **LDPC codes** are now built with progressive edge growth (PEG) and an irregular
  information-degree profile, and decoded with layered offset min-sum (offset 0.5) instead
  of normalized min-sum. At 1 % frame error rate the rate-1/2 code needs 1.5 dB Eb/N0
  instead of 3.1 dB, the header code 2.2 dB instead of 3.7 dB; the other rates gain
  0.3–0.5 dB. The normative construction in `docs/PROTOCOL.md` §2.4 is rewritten.
- **Transmit drive is set per speed level.** BPSK and QPSK bursts go out louder than
  64QAM bursts (wide: RMS 0.50 / 0.40 / 0.30 for levels 0–2 / 3–4 / 5–6; narrow: 0.30 /
  0.30 / 0.25). Peaks are limited by two passes of zero-phase clip-and-filter, so
  out-of-band splatter stays more than 30 dB down. Connect requests and acks are now the
  loudest bursts: calibrate deviation while the modem sends a connect request (see
  `docs/RADIO.md`).
- **Receiver channel estimate** is trained on the decoded header symbols as well as the
  preamble, and a per-symbol noise scale down-weights symbols hit by an FM click or
  squelch pop so one bad symbol no longer sinks a codeword.
- **Rate adaptation** thresholds were re-measured on a 0.25 dB grid. Another level's SNR
  is predicted from the measured burst with a drive-and-ceiling model
  (`1/snr = 1/(channel · drive²) + 1/ceiling`), so the link no longer stalls below the
  fastest level on a strong signal.
- Speed level 6 (64QAM 5/6) decodes error-free from about 16.5 dB carrier-to-noise in
  narrow mode and 24.5 dB in wide mode (was 20 dB and 28 dB). In the simulator a 2 × 10 kB
  transfer at 80 ppm clock error takes 25–40 % less time than 0.2.1 at moderate signal.
- The modem builds each distinct LDPC code once at start-up.

### Known limitations
- Decoding the highest levels costs up to about 45 % more CPU than 0.2.1. Desktop PCs have
  ample headroom; on a Raspberry Pi 3 check `vardefm bench` and use `--max-level` if it
  reports a level as too slow.

## [0.2.1] - 2026-10-17

### Added
- macOS support: release archives for Apple Silicon and Intel, CI tests on macOS.

## [0.2.0] - 2026-10-17

### Added
- Raspberry Pi: GPIO PTT, the `bench` command, multi-core decoding and `--max-level`.
- The user guide ships as `README.md` inside the release archives.

## [0.1.0] - 2026-10-17

First release: real-valued OFDM modem for FM radios (narrow and wide modes), LDPC error
correction with seven speed levels, selective-repeat ARQ link layer, VARA-compatible TCP
host interface, `sim-pair` virtual radio pair, sound card I/O with serial, CM108 and
GPIO PTT, and release builds for Windows, Linux and Raspberry Pi.

[0.3.0]: https://github.com/notepid/vardeFM/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/notepid/vardeFM/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/notepid/vardeFM/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/notepid/vardeFM/releases/tag/v0.1.0
