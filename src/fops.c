#include <stdio.h>

int swap_fd(FILE* file, int fd) {
  int old_fd = fileno(file);
  file->_fileno = fd;

  return old_fd;
}

