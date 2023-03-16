FROM ghcr.io/inko-lang/inko:main as builder
ADD . /openflow
WORKDIR /openflow
RUN /usr/bin/inko build -o openflow.ibi src/main.inko

FROM ghcr.io/inko-lang/inko:main
COPY --from=builder ["/openflow/openflow.ibi", "/openflow/openflow.ibi"]
WORKDIR /openflow
CMD ["/usr/bin/inko", "/openflow/openflow.ibi"]
