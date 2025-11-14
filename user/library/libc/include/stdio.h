#ifndef LIBC_STDIO_H
#define LIBC_STDIO_H

typedef struct __cos_libc_FILE FILE;
typedef long fpos_t;

FILE *__cos_libc_stdin(void);
FILE *__cos_libc_stdout(void);
FILE *__cos_libc_stderr(void);

#define _IOFBF 0
#define _IOLBF 1
#define _IONBF 2
#define BUFSIZ 512
#define EOF -1
#define FOPEN_MAX 16
#define FILENAME_MAX 512
#define L_tmpnam 256
#define SEEK_CUR 1
#define SEEK_END 2
#define SEEK_SET 0
#define TMP_MAX 100

#define stdin (__cos_libc_stdin())
#define stdout (__cos_libc_stdout())
#define stderr (__cos_libc_stderr())

#endif