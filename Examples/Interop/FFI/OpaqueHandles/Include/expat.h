/* Minimal Expat surface used by the opaque-handle example. */
typedef struct XML_ParserStruct *XML_Parser;

XML_Parser XML_ParserCreate(const char *encoding);
int XML_Parse(XML_Parser parser, const char *s, int len, int isFinal);
int XML_GetErrorCode(XML_Parser parser);
unsigned long XML_GetCurrentLineNumber(XML_Parser parser);
unsigned long XML_GetCurrentColumnNumber(XML_Parser parser);
void XML_ParserFree(XML_Parser parser);
