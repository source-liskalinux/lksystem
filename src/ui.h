#ifndef LKSYSTEM_UI_H
#define LKSYSTEM_UI_H

/* Small, allocation-free status UI for native lksystem programs. */
enum lksystem_ui_level {
  LKSYSTEM_UI_INFO = 0,
  LKSYSTEM_UI_SUCCESS = 1,
  LKSYSTEM_UI_WARNING = 2,
  LKSYSTEM_UI_ERROR = 3
};

void lksystem_ui_message(unsigned char level, const char *message);
void lksystem_ui_welcome(void);

void lksystem_ui_info(const char *message);
void lksystem_ui_success(const char *message);
void lksystem_ui_warning(const char *message);
void lksystem_ui_error(const char *message);

#endif
