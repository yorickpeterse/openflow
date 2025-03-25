FROM ghcr.io/inko-lang/inko:main AS builder
ADD . /openflow
WORKDIR /openflow
RUN microdnf install --assumeyes git
RUN inko build --release

FROM ghcr.io/inko-lang/inko:main
COPY --from=builder ["/openflow/build/release/openflow", "/usr/bin/openflow"]
CMD ["/usr/bin/openflow"]
