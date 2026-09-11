#pragma once

/*
 * The executable workflow deliberately does not reimplement libpng.  This
 * header records the narrow native protocol owned by the package adapter:
 * initialization, explicit RGBA selection, row decode, typed error, and close.
 * Unknown operations stay in the lower-level C binding.
 */
struct JetPngProtocol;
struct JetPngImage;

JetPngProtocol* jet_png_open(const char* path);
int jet_png_read_rgba(JetPngProtocol*, JetPngImage*);
void jet_png_close(JetPngProtocol*);
