#ifndef LIBC_ERRNO_H
#define LIBC_ERRNO_H

int *__cos_libc_errno();

#define EDOM 1
#define EILSEQ 2
#define ERANGE 3

#define errno (*__cos_libc_errno())

#endif