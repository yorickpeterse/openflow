FROM alpine:3 AS builder

# Build Inko from source.
RUN apk add --update make libffi libffi-dev rust cargo build-base git
RUN git clone https://gitlab.com/inko-lang/inko.git /inko
WORKDIR /inko

# For some reason the Makefile produces permission denied errors. This has
# something to due with Alpine and Arch Linux, but I can't figure out why, and I
# can't reproduce it elsewhere (e.g. on the Fedora server). So instead we'll
# just run the necessary commands manually.
RUN env INKO_LIBSTD='/usr/lib/inko/libstd' cargo build \
    --release --features libffi-system && \
    strip target/release/inko

RUN mkdir -p /usr/lib/inko/libstd && \
    cp -r libstd/src/* /usr/lib/inko/libstd && \
    install -m755 target/release/inko /usr/bin/inko

# Build Openflow
ADD . /openflow
WORKDIR /openflow
RUN /usr/bin/inko build -o openflow.ibi src/main.inko

FROM alpine:3

COPY --from=builder ["/usr/bin/inko", "/usr/bin/inko"]
COPY --from=builder ["/usr/lib/inko", "/usr/lib/inko/"]
COPY --from=builder ["/openflow/openflow.ibi", "/openflow/openflow.ibi"]

# libgcc is needed because libgcc is dynamically linked to the executable.
RUN apk add --update libffi libffi-dev libgcc

WORKDIR /openflow
CMD ["/usr/bin/inko", "/openflow/openflow.ibi"]
