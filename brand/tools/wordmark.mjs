import { parse, Path } from "opentype.js";
import * as hb from "harfbuzzjs";

export function wordmark(bytes, size = 200) {
  const source = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
  const face = parse(source);
  const font = new hb.Font(new hb.Face(new hb.Blob(source)));
  const buffer = new hb.Buffer();
  buffer.addText("Kossé");
  buffer.guessSegmentProperties();
  // Kibernetyk decomposes é through ccmp and positions its acute through GPOS.
  // Direct character-to-glyph outline lookup silently drops the entire letter.
  hb.shape(font, buffer);
  const glyphs = buffer.getGlyphInfos();
  const positions = buffer.getGlyphPositions();
  const path = new Path();
  const scale = size / face.unitsPerEm;
  let x = 0;
  for (const [i, info] of glyphs.entries()) {
    const position = positions[i];
    path.extend(
      face.glyphs
        .get(info.codepoint)
        .getPath((x + position.xOffset) * scale, -position.yOffset * scale, size),
    );
    x += position.xAdvance;
  }
  return { face, path, glyphs, positions };
}
