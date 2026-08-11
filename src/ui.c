#include <ctype.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include "ui.h"

#define UI_RESET "\033[0m"
#define UI_CYAN "\033[1;36m"
#define UI_GREEN "\033[1;32m"
#define UI_YELLOW "\033[1;33m"
#define UI_RED "\033[1;31m"

static int ui_colour_enabled(void) {
  return isatty(STDERR_FILENO);
}

static void ui_emit(unsigned char level, const char *message) {
  const char *prefix;
  const char *colour;
  int colour_enabled;

  if (message == 0) return;

  switch (level) {
  case LKSYSTEM_UI_INFO:
    prefix = "[i]";
    colour = UI_CYAN;
    break;
  case LKSYSTEM_UI_SUCCESS:
    prefix = "[✓]";
    colour = UI_GREEN;
    break;
  case LKSYSTEM_UI_WARNING:
    prefix = "[!]";
    colour = UI_YELLOW;
    break;
  default:
    prefix = "[✗]";
    colour = UI_RED;
    break;
  }

  colour_enabled = ui_colour_enabled();
  if (colour_enabled) fputs(colour, stderr);
  fputs(prefix, stderr);
  if (colour_enabled) fputs(UI_RESET, stderr);
  fputc(' ', stderr);
  if (colour_enabled && level != LKSYSTEM_UI_INFO) fputs(colour, stderr);
  fputs(message, stderr);
  if (colour_enabled && level != LKSYSTEM_UI_INFO) fputs(UI_RESET, stderr);
  fputc('\n', stderr);
}

static void ui_copy_value(char *destination, size_t destination_size,
                          const char *value) {
  size_t length;
  size_t output = 0;

  while (isspace((unsigned char)*value)) value++;
  length = strlen(value);
  while (length > 0 && isspace((unsigned char)value[length - 1])) length--;
  if (length >= 2 && value[0] == '"' && value[length - 1] == '"') {
    value++;
    length -= 2;
  }

  while (length > 0 && output + 1 < destination_size) {
    if (value[0] == '\\' && length > 1 &&
        (value[1] == '\\' || value[1] == '"')) {
      value++;
      length--;
    }
    destination[output++] = *value++;
    length--;
  }
  destination[output] = 0;
}

static int ui_find_os_release_value(const char *path, const char *key,
                                    char *name, size_t name_size) {
  FILE *file;
  char line[1024];
  size_t key_length;

  if (name_size == 0) return 0;
  name[0] = 0;
  file = fopen(path, "r");
  if (file == 0) return 0;
  key_length = strlen(key);

  while (fgets(line, sizeof line, file) != 0) {
    if (strncmp(line, key, key_length) == 0 && line[key_length] == '=') {
      ui_copy_value(name, name_size, line + key_length + 1);
      fclose(file);
      return name[0] != 0;
    }
  }
  fclose(file);
  return 0;
}

static void ui_os_name(char *name, size_t name_size) {
  if (ui_find_os_release_value("/etc/os-release", "PRETTY_NAME", name,
                               name_size)) return;
  if (ui_find_os_release_value("/etc/os-release", "NAME", name, name_size))
    return;
  if (name_size != 0) {
    strncpy(name, "Linux", name_size - 1);
    name[name_size - 1] = 0;
  }
}

void lksystem_ui_message(unsigned char level, const char *message) {
  ui_emit(level, message);
}

void lksystem_ui_welcome(void) {
  char name[1024];
  int colour_enabled = ui_colour_enabled();

  ui_os_name(name, sizeof name);
  if (colour_enabled) fputs(UI_CYAN, stderr);
  fputs("::: [ Welcome To ", stderr);
  fputs(name, stderr);
  fputs(" ] :::", stderr);
  if (colour_enabled) fputs(UI_RESET, stderr);
  fputc('\n', stderr);
}

void lksystem_ui_info(const char *message) {
  lksystem_ui_message(LKSYSTEM_UI_INFO, message);
}

void lksystem_ui_success(const char *message) {
  lksystem_ui_message(LKSYSTEM_UI_SUCCESS, message);
}

void lksystem_ui_warning(const char *message) {
  lksystem_ui_message(LKSYSTEM_UI_WARNING, message);
}

void lksystem_ui_error(const char *message) {
  lksystem_ui_message(LKSYSTEM_UI_ERROR, message);
}
