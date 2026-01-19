load("@prelude//mdbook:mdbook.bzl", "mdbook_book")

mdbook_book(
    name = "developer-manual",
    book_toml = "book.toml",
    srcs = glob(["src/**/*.md"]),
)
