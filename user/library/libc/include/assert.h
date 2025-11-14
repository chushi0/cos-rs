#ifndef LIBC_ASSERT_H
#define LIBC_ASSERT_H

void __cos_libc_assert_fail(const char *expr, const char *file, int line, const char *func);

#ifdef NDEBUG
#define assert(x) ((void)0)
#else
#define assert(x) (!!(x) ? void(0) : __cos_libc_assert_fail(#x, __FILE__, __LINE__, __func__))
#endif

#endif