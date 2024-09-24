FROM ghcr.io/inko-lang/inko:main AS builder
ADD . /openflow
WORKDIR /openflow
RUN microdnf install --assumeyes git
RUN inko build -o build/openflow

FROM ghcr.io/inko-lang/inko:main
COPY --from=builder ["/openflow/build/openflow", "/usr/bin/openflow"]
CMD ["/usr/bin/openflow"]
