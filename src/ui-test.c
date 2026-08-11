#include <stdio.h>
#include "ui.h"

int main(void) {
  lksystem_ui_message(LKSYSTEM_UI_INFO, 0);
  lksystem_ui_info("native C UI smoke test");
  lksystem_ui_success("success");
  lksystem_ui_warning("warning");
  lksystem_ui_error("error");
  lksystem_ui_welcome();
  return ferror(stderr) ? 1 : 0;
}
