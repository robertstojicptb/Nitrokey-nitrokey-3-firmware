RUNNER := runners/lpc55

build:
	make -C $(RUNNER) build

build-dev:
	make -C $(RUNNER) build DEVELOP=1

bacon:
	make -C $(RUNNER) bacon

run:
	make -C $(RUNNER) run

jlink:
	scripts/bump-jlink
	JLinkGDBServer -strict -device LPC55S69 -if SWD -vd

mount-fs:
	scripts/fuse-bee

umount-fs:
	scripts/defuse-bee

.PHONY: clean
clean:
	make -C $(RUNNER) clean
