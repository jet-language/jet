// The runtime generators' process-static text: `rt_static_text("...")` for a
// literal longer than two machine words, whose bytes the image holds once and
// a report writes or pushes in one call. Up to two words, the word constants
// cost less than the text's 24-byte String header plus its padded bytes.
export function staticTextWorthy(text) {
  return Buffer.byteLength(text) > 16;
}

export function staticText(text) {
  if (/[\x00-\x09\x0b-\x1f\x7f]/.test(text)) throw new Error(`control byte in runtime static text ${JSON.stringify(text)}`);
  const body = text.replaceAll("\\", "\\\\").replaceAll("\"", "\\\"").replaceAll("{", "{{").replaceAll("}", "}}").replaceAll("\n", "\\n");
  return `rt_static_text("${body}")`;
}
