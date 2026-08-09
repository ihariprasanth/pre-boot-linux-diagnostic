.PHONY: agent initramfs iso kernel server dashboard qemu test dev clean

# ---- Diagnostic Linux side ----

agent:
	cd diagnostic/agent && RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target x86_64-unknown-linux-gnu

agent-test:
	cd diagnostic/agent && cargo test

kernel:
	bash diagnostic/build/build-kernel.sh

initramfs: agent
	bash diagnostic/build/build-initramfs.sh

iso: kernel initramfs
	bash diagnostic/build/build-iso.sh

# ---- Backend / dashboard ----

server:
	@echo "[stub] Phase 5: start FastAPI backend (server/)"

dashboard:
	cd dashboard && npm install && npm run dev

# ---- Testing ----

qemu: initramfs
	bash scripts/run-qemu.sh

test:
	bash scripts/test.sh

dev:
	@echo "[stub] spin up local dev environment (scripts/dev.sh)"

clean:
	@echo "[stub] remove build artifacts"
