/*
 * common_rust.h — Drop-in replacement for common.h
 *
 * Used when linking tests against libcjson_rs.a (Rust) instead of cJSON.c.
 *
 * Key differences from common.h:
 *   1. Includes cJSON.h (header only), NOT cJSON.c (the full source).
 *   2. Links against the Rust static library for: cJSON_Parse, cJSON_Delete,
 *      cJSON_InitHooks.
 *   3. Links against a modified cJSON.c object for all remaining functions
 *      (cJSON_Print, cJSON_Create*, cJSON_Get*, etc.) with the Rust-ported
 *      functions stubbed out via preprocessor to avoid duplicate symbols.
 *   4. The reset() function uses free() (stdlib) instead of the internal
 *      global_hooks.deallocate — this matches the Rust allocator's behavior
 *      since our Rust cJSON_Parse allocates strings via CString (which uses
 *      the system allocator).
 */

#ifndef CJSON_TESTS_COMMON_RUST_H
#define CJSON_TESTS_COMMON_RUST_H

#include "../cJSON.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdbool.h>
#include <math.h>
#include <float.h>

/*
 * compare_double() -- from cJSON.c (static function).
 * Needed by readme_examples.c which originally got it via #include "../cJSON.c".
 */
static cJSON_bool compare_double(double a, double b)
{
    double maxVal = fabs(a) > fabs(b) ? fabs(a) : fabs(b);
    return (fabs(a - b) <= maxVal * DBL_EPSILON);
}

/*
 * reset() -- clean up a stack-allocated cJSON item.
 *
 * The original common.h version uses global_hooks.deallocate (an internal
 * C static). We use free() instead, which is correct because:
 *   - Strings allocated by the Rust cJSON_Parse use CString::into_raw(),
 *     which allocates via the system allocator (compatible with free()).
 *   - Strings allocated by C functions (cJSON_CreateString, cJSON_Print,
 *     etc.) also use the default hooks (malloc/free) unless custom hooks
 *     are installed.
 */
void reset(cJSON *item);
void reset(cJSON *item) {
    if ((item != NULL) && (item->child != NULL))
    {
        cJSON_Delete(item->child);
    }
    if ((item->valuestring != NULL) && !(item->type & cJSON_IsReference))
    {
        free(item->valuestring);
    }
    if ((item->string != NULL) && !(item->type & cJSON_StringIsConst))
    {
        free(item->string);
    }

    memset(item, 0, sizeof(cJSON));
}

/* read_file() — identical to the original common.h version */
char* read_file(const char *filename);
char* read_file(const char *filename) {
    FILE *file = NULL;
    long length = 0;
    char *content = NULL;
    size_t read_chars = 0;

    /* open in read binary mode */
    file = fopen(filename, "rb");
    if (file == NULL)
    {
        goto cleanup;
    }

    /* get the length */
    if (fseek(file, 0, SEEK_END) != 0)
    {
        goto cleanup;
    }
    length = ftell(file);
    if (length < 0)
    {
        goto cleanup;
    }
    if (fseek(file, 0, SEEK_SET) != 0)
    {
        goto cleanup;
    }

    /* allocate content buffer */
    content = (char*)malloc((size_t)length + sizeof(""));
    if (content == NULL)
    {
        goto cleanup;
    }

    /* read the file into memory */
    read_chars = fread(content, sizeof(char), (size_t)length, file);
    if ((long)read_chars != length)
    {
        free(content);
        content = NULL;
        goto cleanup;
    }
    content[read_chars] = '\0';


cleanup:
    if (file != NULL)
    {
        fclose(file);
    }

    return content;
}

/* assertion helper macros — identical to original common.h */
#define assert_has_type(item, item_type) TEST_ASSERT_BITS_MESSAGE(0xFF, item_type, item->type, "Item doesn't have expected type.")
#define assert_has_no_reference(item) TEST_ASSERT_BITS_MESSAGE(cJSON_IsReference, 0, item->type, "Item should not have a string as reference.")
#define assert_has_no_const_string(item) TEST_ASSERT_BITS_MESSAGE(cJSON_StringIsConst, 0, item->type, "Item should not have a const string.")
#define assert_has_valuestring(item) TEST_ASSERT_NOT_NULL_MESSAGE(item->valuestring, "Valuestring is NULL.")
#define assert_has_no_valuestring(item) TEST_ASSERT_NULL_MESSAGE(item->valuestring, "Valuestring is not NULL.")
#define assert_has_string(item) TEST_ASSERT_NOT_NULL_MESSAGE(item->string, "String is NULL")
#define assert_has_no_string(item) TEST_ASSERT_NULL_MESSAGE(item->string, "String is not NULL.")
#define assert_not_in_list(item) \
	TEST_ASSERT_NULL_MESSAGE(item->next, "Linked list next pointer is not NULL.");\
	TEST_ASSERT_NULL_MESSAGE(item->prev, "Linked list previous pointer is not NULL.")
#define assert_has_child(item) TEST_ASSERT_NOT_NULL_MESSAGE(item->child, "Item doesn't have a child.")
#define assert_has_no_child(item) TEST_ASSERT_NULL_MESSAGE(item->child, "Item has a child.")
#define assert_is_invalid(item) \
	assert_has_type(item, cJSON_Invalid);\
	assert_not_in_list(item);\
	assert_has_no_child(item);\
	assert_has_no_string(item);\
	assert_has_no_valuestring(item)

#endif
