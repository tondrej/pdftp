# pdftp

PDF template processor inspired by [pdfium][1]'s [fixup_pdf_template][2]

Expands a hand-written PDF testcase (template) into a valid PDF file.

There are several places in a PDF file where byte-offsets are required. This
tool replaces `{{name}}`-style tags in the input with calculated results.

| tag                                  | description                                                                                                     |
|--------------------------------------|-----------------------------------------------------------------------------------------------------------------|
| `{{if name}}`/`{{else}}`/`{{endif}}` | conditional processing depending on existence of the named variable.                                            |
| `{{include path/to/file}}`           | inserts and processes another template file.                                                                    |
| `{{includeraw path/to/file}}`        | inserts file's contents into stream as raw binary data.                                                         |
| `{{header}}`                         | expands to the header comment required for PDF files.                                                           |
| `{{xref}}`                           | expands to a generated xref table, noting the offset.                                                           |
| `{trailer}}`                         | expands to a standard trailer with reference to catalog as the `/Root`. (`catalog` dictionary must be defined.) |
| `{{trailersize}}`                    | expands to `/Size n`, to be used in non-standard trailers.                                                      |
| `{{startxref}`                       | expands to a startxref directive followed by correct offset.                                                    |
| `{{startxrefobj x y}`                | expands to a `startxref` directive followed by correct offset pointing to the start of `x y obj`.               |
| `{{object x y}}`                     | expands to `x y obj` declaration, noting the offset.                                                            |
| `{{object alias}}`                   | expands to `x y obj` declaration of the object with the given alias.                                            |
| `{{ref alias}}`                      | expands to `x y R` reference to the object with the given alias.                                                |
| `{{streamlen}}`                      | expands to `/Length n` where n is the length of the enclosing `stream`.                                         |
| `{{var name}}`                       | expands to value of the named variable.                                                                         |

See examples in the [data][3] directory.

[1]: https://pdfium.googlesource.com
[2]: https://pdfium.googlesource.com/pdfium/+/refs/heads/main/testing/tools/fixup_pdf_template.py
[3]: data/