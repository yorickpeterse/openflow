FROM ghcr.io/inko-lang/inko:latest AS builder
ADD . /openflow
WORKDIR /openflow
RUN inko build -o build/openflow

FROM ghcr.io/inko-lang/inko:latest
COPY --from=builder ["/openflow/build/openflow", "/usr/bin/openflow"]
CMD ["/usr/bin/openflow"]
