#!/usr/bin/env python3
"""The appendix C.2 screenshot parity metric.

Appendix C.2, step 4, is the sentence this tool implements, quoted verbatim:

    Compare side by side, then with a per-pixel difference metric. Tolerance:
    no more than 2% of pixels differing by more than 8/255 per channel, with an
    absolute cap of 1% differing by more than 24/255. Differences caused by
    animated textures or entity positions are excluded by freezing the world
    (/gamerule doDaylightCycle false, mobs absent or stationary,
    randomTickSpeed 0).

The tool reads a pair of 8-bit RGB or RGBA non-interlaced PNGs (all five PNG
filter types), counts the pixels whose largest per-channel difference exceeds
8 and 24, and reports the counts, the fractions and the verdict. Rectangular
masks may exclude regions the procedure itself excludes (animated textures,
the vanilla HUD when it cannot be hidden, the cloud band); the masked metric
drops the masked pixels from both the numerator and the denominator, so the
masked numbers are the metric over the unmasked pixels only.

One JSON object is printed per pair: the unmasked metric, the masked metric,
the masks used, and `pass`, true when `over_8_frac <= 0.02` and
`over_24_frac <= 0.01`. `--self-test` builds fixture PNGs in a temporary
directory and asserts the metric, the verdicts and the reader against known
counts, including a failing pair and the exact-tolerance boundary.

Standard library only: zlib, struct, argparse, json.
"""

import argparse
import json
import os
import struct
import sys
import tempfile
import zlib

PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"

# The C.2 step 4 tolerance, kept as the two numbers the sentence names.
OVER_8_LIMIT = 0.02
OVER_24_LIMIT = 0.01
# A channel difference is counted when it is strictly greater than the bound.
OVER_8_BOUND = 8
OVER_24_BOUND = 24


class PngError(Exception):
    """A PNG the reader cannot accept, with the reason."""


def _paeth(a, b, c):
    """The Paeth predictor: the neighbour a, b or c closest to a + b - c."""
    p = a + b - c
    pa = abs(p - a)
    pb = abs(p - b)
    pc = abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    if pb <= pc:
        return b
    return c


def _unfilter(raw, width, height, channels):
    """Reverses the five PNG row filters into packed 8-bit samples."""
    stride = width * channels
    if len(raw) != (stride + 1) * height:
        raise PngError(
            "the image data is %d bytes, expected %d"
            % (len(raw), (stride + 1) * height)
        )
    out = bytearray(stride * height)
    prev = bytearray(stride)
    pos = 0
    for row_index in range(height):
        filter_type = raw[pos]
        pos += 1
        row = bytearray(raw[pos : pos + stride])
        pos += stride
        if filter_type == 0:  # None
            pass
        elif filter_type == 1:  # Sub
            for i in range(channels, stride):
                row[i] = (row[i] + row[i - channels]) & 0xFF
        elif filter_type == 2:  # Up
            for i in range(stride):
                row[i] = (row[i] + prev[i]) & 0xFF
        elif filter_type == 3:  # Average
            for i in range(stride):
                left = row[i - channels] if i >= channels else 0
                row[i] = (row[i] + ((left + prev[i]) >> 1)) & 0xFF
        elif filter_type == 4:  # Paeth
            for i in range(stride):
                left = row[i - channels] if i >= channels else 0
                up = prev[i]
                up_left = prev[i - channels] if i >= channels else 0
                row[i] = (row[i] + _paeth(left, up, up_left)) & 0xFF
        else:
            raise PngError("row %d has filter type %d" % (row_index, filter_type))
        out[row_index * stride : (row_index + 1) * stride] = row
        prev = row
    return bytes(out)


def read_png(path):
    """Reads an 8-bit RGB or RGBA non-interlaced PNG.

    Returns `(width, height, channels, data)` with packed samples, three
    channels for colour type 2 and four for colour type 6.
    """
    with open(path, "rb") as handle:
        blob = handle.read()
    if blob[:8] != PNG_SIGNATURE:
        raise PngError("%s is not a PNG (bad signature)" % path)
    pos = 8
    header = None
    idat = bytearray()
    while pos < len(blob):
        if pos + 8 > len(blob):
            raise PngError("%s ends inside a chunk header" % path)
        (length,) = struct.unpack(">I", blob[pos : pos + 4])
        tag = blob[pos + 4 : pos + 8]
        payload = blob[pos + 8 : pos + 8 + length]
        if len(payload) != length:
            raise PngError("%s ends inside a %r chunk" % (path, tag))
        if tag == b"IHDR":
            header = struct.unpack(">IIBBBBB", payload)
        elif tag == b"IDAT":
            idat += payload
        elif tag == b"IEND":
            break
        pos += 12 + length
    if header is None:
        raise PngError("%s has no IHDR chunk" % path)
    width, height, depth, colour, compression, filter_method, interlace = header
    if depth != 8:
        raise PngError("%s: bit depth %d, only 8 is supported" % (path, depth))
    if colour not in (2, 6):
        raise PngError(
            "%s: colour type %d, only 2 (RGB) and 6 (RGBA) are supported"
            % (path, colour)
        )
    if compression != 0 or filter_method != 0:
        raise PngError("%s: unsupported compression or filter method" % path)
    if interlace != 0:
        raise PngError("%s: interlaced images are not supported" % path)
    if width == 0 or height == 0:
        raise PngError("%s: zero width or height" % path)
    channels = 3 if colour == 2 else 4
    raw = zlib.decompress(bytes(idat))
    return width, height, channels, _unfilter(raw, width, height, channels)


def write_png(path, width, height, channels, data):
    """Writes an 8-bit RGB/RGBA PNG with filter 0 on every row."""
    if channels not in (3, 4):
        raise PngError("channels must be 3 or 4, got %d" % channels)
    stride = width * channels
    if len(data) != stride * height:
        raise PngError("the image data does not match %dx%d" % (width, height))
    raw = bytearray()
    for row in range(height):
        raw.append(0)
        raw += data[row * stride : (row + 1) * stride]

    def chunk(tag, payload):
        return (
            struct.pack(">I", len(payload))
            + tag
            + payload
            + struct.pack(">I", zlib.crc32(tag + payload) & 0xFFFFFFFF)
        )

    colour = 2 if channels == 3 else 6
    with open(path, "wb") as handle:
        handle.write(PNG_SIGNATURE)
        handle.write(
            chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, colour, 0, 0, 0))
        )
        handle.write(chunk(b"IDAT", zlib.compress(bytes(raw))))
        handle.write(chunk(b"IEND", b""))


def parse_mask(text, label):
    """Parses `x,y,w,h` (commas or spaces) into a mask tuple."""
    parts = text.replace(",", " ").split()
    if len(parts) != 4:
        raise PngError("a mask must be x,y,w,h, got %r" % text)
    try:
        x, y, w, h = (int(part) for part in parts)
    except ValueError:
        raise PngError("a mask must be four integers, got %r" % text)
    if w <= 0 or h <= 0:
        raise PngError("a mask needs a positive width and height, got %r" % text)
    return {"x": x, "y": y, "w": w, "h": h, "label": label}


def read_mask_file(path):
    """Reads one `x y w h label` mask per line; `#` starts a comment."""
    masks = []
    with open(path, "r", encoding="utf-8") as handle:
        for number, line in enumerate(handle, start=1):
            line = line.split("#", 1)[0].strip()
            if not line:
                continue
            parts = line.split(None, 4)
            if len(parts) < 4:
                raise PngError("%s:%d: expected x y w h [label]" % (path, number))
            try:
                x, y, w, h = (int(part) for part in parts[:4])
            except ValueError:
                raise PngError("%s:%d: x y w h must be integers" % (path, number))
            label = parts[4].strip() if len(parts) == 5 else "mask-%d" % number
            masks.append(parse_mask("%d %d %d %d" % (x, y, w, h), label))
    return masks


def mask_coverage(width, height, masks):
    """A byte per pixel: 1 under any mask, 0 outside every mask.

    Masks are clipped to the image; a mask wholly outside contributes nothing.
    """
    covered = bytearray(width * height)
    for mask in masks:
        x0 = max(0, mask["x"])
        y0 = max(0, mask["y"])
        x1 = min(width, mask["x"] + mask["w"])
        y1 = min(height, mask["y"] + mask["h"])
        for y in range(y0, y1):
            covered[y * width + x0 : y * width + x1] = b"\x01" * (x1 - x0)
    return covered


def compare_png(a_path, b_path, masks):
    """The C.2 metric for one pair, with and without the masks."""
    width, height, channels, a = read_png(a_path)
    b_width, b_height, b_channels, b = read_png(b_path)
    if (width, height, channels) != (b_width, b_height, b_channels):
        raise PngError(
            "the pair does not share a size or format: %s is %dx%d/%d channels, "
            "%s is %dx%d/%d channels"
            % (a_path, width, height, channels, b_path, b_width, b_height, b_channels)
        )
    total = width * height
    covered = mask_coverage(width, height, masks)
    masked_total = total - sum(covered)
    over_8 = 0
    over_24 = 0
    masked_over_8 = 0
    masked_over_24 = 0
    for pixel in range(total):
        base = pixel * channels
        difference = 0
        for channel in range(channels):
            delta = abs(a[base + channel] - b[base + channel])
            if delta > difference:
                difference = delta
        if difference > OVER_8_BOUND:
            over_8 += 1
        if difference > OVER_24_BOUND:
            over_24 += 1
        if not covered[pixel]:
            if difference > OVER_8_BOUND:
                masked_over_8 += 1
            if difference > OVER_24_BOUND:
                masked_over_24 += 1

    def numbers(pixels, count_8, count_24):
        return {
            "pixels_total": pixels,
            "pixels_over_8": count_8,
            "over_8_frac": round(count_8 / pixels, 6) if pixels else 0.0,
            "pixels_over_24": count_24,
            "over_24_frac": round(count_24 / pixels, 6) if pixels else 0.0,
            "pass": (
                (count_8 / pixels) <= OVER_8_LIMIT
                and (count_24 / pixels) <= OVER_24_LIMIT
                if pixels
                else True
            ),
        }

    report = {
        "pair": [os.fspath(a_path), os.fspath(b_path)],
        "width": width,
        "height": height,
        "channels": channels,
        "masks": masks,
        "pixels_masked": total - masked_total,
    }
    report.update(numbers(total, over_8, over_24))
    report["masked"] = numbers(masked_total, masked_over_8, masked_over_24)
    return report


def _solid(width, height, channels, colour):
    data = bytearray(width * height * channels)
    for pixel in range(width * height):
        for channel in range(channels):
            data[pixel * channels + channel] = colour[channel]
    return data


def _patch(data, width, channels, x0, y0, w, h, colour):
    """Sets a w x h block at (x0, y0) to `colour` (three or four samples)."""
    for y in range(y0, y0 + h):
        for x in range(x0, x0 + w):
            base = (y * width + x) * channels
            for channel, value in enumerate(colour):
                data[base + channel] = value


def _encode_filtered_row(filter_type, row, prev, channels):
    """Encodes one row with a chosen filter, for the reader's filter test."""
    out = bytearray(row)
    for i in range(len(row)):
        left = row[i - channels] if i >= channels else 0
        up = prev[i] if prev is not None else 0
        up_left = prev[i - channels] if (prev is not None and i >= channels) else 0
        if filter_type == 0:
            predictor = 0
        elif filter_type == 1:
            predictor = left
        elif filter_type == 2:
            predictor = up
        elif filter_type == 3:
            predictor = (left + up) >> 1
        elif filter_type == 4:
            predictor = _paeth(left, up, up_left)
        else:
            raise PngError("unknown filter type %d" % filter_type)
        out[i] = (row[i] - predictor) & 0xFF
    return bytes([filter_type]) + bytes(out)


def _write_png_with_filters(path, width, height, channels, data, filters):
    """Writes a PNG whose rows use the given filter types, for the self-test."""
    stride = width * channels
    raw = bytearray()
    prev = None
    for row_index in range(height):
        row = data[row_index * stride : (row_index + 1) * stride]
        raw += _encode_filtered_row(filters[row_index], row, prev, channels)
        prev = row

    def chunk(tag, payload):
        return (
            struct.pack(">I", len(payload))
            + tag
            + payload
            + struct.pack(">I", zlib.crc32(tag + payload) & 0xFFFFFFFF)
        )

    colour = 2 if channels == 3 else 6
    with open(path, "wb") as handle:
        handle.write(PNG_SIGNATURE)
        handle.write(
            chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, colour, 0, 0, 0))
        )
        handle.write(chunk(b"IDAT", zlib.compress(bytes(raw))))
        handle.write(chunk(b"IEND", b""))


def self_test():
    """Builds fixture PNGs and asserts the metric, the verdicts and the reader."""
    failures = []
    checks = 0

    def check(name, condition, detail=""):
        nonlocal checks
        checks += 1
        if condition:
            print("ok   %s" % name)
        else:
            print("FAIL %s %s" % (name, detail))
            failures.append(name)

    directory = tempfile.mkdtemp(prefix="parity-diff-selftest-")

    def path(name):
        return os.path.join(directory, name)

    def build(name, width, height, channels, colour, patch=None, patches=()):
        data = _solid(width, height, channels, colour)
        if patch is not None:
            x0, y0, w, h, patch_colour = patch
            _patch(data, width, channels, x0, y0, w, h, patch_colour)
        for x0, y0, w, h, patch_colour in patches:
            _patch(data, width, channels, x0, y0, w, h, patch_colour)
        write_png(path(name), width, height, channels, data)

    base_colour = (40, 80, 120)
    # 1. An identical pair.
    build("same-a.png", 100, 100, 3, base_colour)
    build("same-b.png", 100, 100, 3, base_colour)
    report = compare_png(path("same-a.png"), path("same-b.png"), [])
    check("identical pair has no differences", report["pixels_over_8"] == 0)
    check("identical pair passes", report["pass"] is True)

    # 2. 1% of the pixels differ by 10 in one channel: over 8, not over 24.
    build("one-percent-a.png", 100, 100, 3, base_colour)
    build(
        "one-percent-b.png",
        100,
        100,
        3,
        base_colour,
        patch=(10, 10, 10, 10, (50, 80, 120)),
    )
    report = compare_png(path("one-percent-a.png"), path("one-percent-b.png"), [])
    check(
        "1% over-10 counts 100 over-8 pixels",
        report["pixels_over_8"] == 100,
        report,
    )
    check(
        "1% over-10 has no over-24 pixels",
        report["pixels_over_24"] == 0,
        report,
    )
    check(
        "1% over-10 fraction is 0.01",
        abs(report["over_8_frac"] - 0.01) < 1e-9,
        report,
    )
    check("1% over-10 passes", report["pass"] is True)

    # 3. 0.5% of the pixels differ by 30: both counts move.
    build("half-percent-a.png", 200, 100, 3, base_colour)
    build(
        "half-percent-b.png",
        200,
        100,
        3,
        base_colour,
        patch=(0, 0, 10, 10, (70, 80, 120)),
    )
    report = compare_png(path("half-percent-a.png"), path("half-percent-b.png"), [])
    check(
        "0.5% over-30 counts 100 over-8 and 100 over-24 pixels",
        report["pixels_over_8"] == 100 and report["pixels_over_24"] == 100,
        report,
    )
    check(
        "0.5% over-30 fraction is 0.005",
        abs(report["over_24_frac"] - 0.005) < 1e-9,
        report,
    )
    check("0.5% over-30 passes", report["pass"] is True)

    # 4. A masked case: 2% over-24 unmasked fails, and a mask over the block
    #    drops the pixels from both sides of the fraction.
    build("masked-a.png", 100, 100, 3, base_colour)
    build(
        "masked-b.png",
        100,
        100,
        3,
        base_colour,
        patch=(20, 20, 20, 10, (70, 80, 120)),
    )
    mask = parse_mask("20,20,20,10", "the changed block")
    report = compare_png(path("masked-a.png"), path("masked-b.png"), [])
    check("the unmasked masked case fails", report["pass"] is False, report)
    report = compare_png(path("masked-a.png"), path("masked-b.png"), [mask])
    check(
        "the mask drops the changed pixels",
        report["masked"]["pixels_over_8"] == 0
        and report["masked"]["pixels_over_24"] == 0,
        report,
    )
    check(
        "the masked metric counts 9800 pixels",
        report["masked"]["pixels_total"] == 9800 and report["pixels_masked"] == 200,
        report,
    )
    check("the masked case passes", report["masked"]["pass"] is True)

    # 5. A failing pair: 3% differ by 10.
    build("fail-a.png", 100, 100, 3, base_colour)
    build(
        "fail-b.png",
        100,
        100,
        3,
        base_colour,
        patch=(0, 0, 30, 10, (50, 80, 120)),
    )
    report = compare_png(path("fail-a.png"), path("fail-b.png"), [])
    check(
        "3% over-8 counts 300 pixels",
        report["pixels_over_8"] == 300,
        report,
    )
    check("3% over-8 fails", report["pass"] is False)

    # 6. The boundary: exactly 2% over-8, of which exactly 1% over-24.
    build("boundary-a.png", 100, 100, 3, base_colour)
    build(
        "boundary-b.png",
        100,
        100,
        3,
        base_colour,
        patches=[
            (0, 0, 10, 10, (70, 80, 120)),
            (10, 0, 10, 10, (50, 80, 120)),
        ],
    )
    report = compare_png(path("boundary-a.png"), path("boundary-b.png"), [])
    check(
        "exactly 2% over-8 and 1% over-24 is the boundary",
        report["pixels_over_8"] == 200
        and report["pixels_over_24"] == 100
        and report["over_8_frac"] == OVER_8_LIMIT
        and report["over_24_frac"] == OVER_24_LIMIT,
        report,
    )
    check("the boundary passes", report["pass"] is True)

    # 7. The reader: RGBA and all five row filters.
    data = bytearray()
    for y in range(5):
        for x in range(8):
            data += bytes(
                ((x * 17 + y) & 0xFF, (x * 31 + y * 7) & 0xFF, (y * 53) & 0xFF, 255)
            )
    _write_png_with_filters(path("filters.png"), 8, 5, 4, bytes(data), [0, 1, 2, 3, 4])
    width, height, channels, read_back = read_png(path("filters.png"))
    check(
        "all five filter types decode",
        (width, height, channels) == (8, 5, 4) and read_back == bytes(data),
        (width, height, channels, read_back[:16]),
    )

    # 8. A malformed file is refused with a message, not a traceback.
    try:
        read_png(path("same-a.png") + ".missing")
        check("a missing file is refused", False)
    except OSError:
        check("a missing file is refused", True)

    if failures:
        print("self-test: %d checks, %d failed: %s" % (checks, len(failures), failures))
        return 1
    print("self-test: %d checks OK" % checks)
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="Appendix C.2 screenshot parity metric (step 4)."
    )
    parser.add_argument("a", nargs="?", help="the first PNG of the pair")
    parser.add_argument("b", nargs="?", help="the second PNG of the pair")
    parser.add_argument(
        "--mask",
        action="append",
        default=[],
        metavar="x,y,w,h",
        help="exclude a rectangle (repeatable)",
    )
    parser.add_argument(
        "--mask-file",
        help="a file with one 'x y w h label' mask per line",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="build fixture PNGs in a temp directory and assert the metric",
    )
    args = parser.parse_args(argv)

    if args.self_test:
        return self_test()
    if args.a is None or args.b is None:
        parser.error("a pair of PNG paths is required (or --self-test)")

    try:
        masks = [parse_mask(text, "mask-%d" % (i + 1)) for i, text in enumerate(args.mask)]
        if args.mask_file:
            masks += read_mask_file(args.mask_file)
        report = compare_png(args.a, args.b, masks)
    except (PngError, OSError) as error:
        print("parity-diff: %s" % error, file=sys.stderr)
        return 2
    print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
