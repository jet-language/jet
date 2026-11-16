const char* good(void){return "caf\xC3\xA9";} const char* null_s(void){return 0;} const char* bad(void){static const char s[]={ (char)0xff,0 };return s;}
