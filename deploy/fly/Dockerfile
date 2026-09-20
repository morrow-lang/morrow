# The image contains only the self-contained static server built by
# `MORROW_WEB_TARGET=x86_64-unknown-linux-musl cargo xtask web-build`.
# Browser assets, the compiled Morrow application and the service worker are
# embedded in that executable; nothing else is needed at runtime.
FROM scratch
COPY dist/morrow-web-linux-x86_64 /morrow-web
EXPOSE 8080
ENTRYPOINT ["/morrow-web"]
