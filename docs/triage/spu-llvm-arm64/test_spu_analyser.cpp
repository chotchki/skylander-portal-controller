#include <gtest/gtest.h>

#include <array>
#include <map>
#include <vector>

#include "util/types.hpp"
#include "Emu/Cell/SPURecompiler.h"
#include "Emu/Cell/SPUThread.h"
#include "Emu/system_config.h"
#include "Emu/system_config_types.h"

// Giga SPU analyser regression: a brsl whose return address is a stop-trap leaves
// a dangling target edge after block cleanup, which the reg-state walk must not
// dereference. The SPU program below is made-up data (not from any game).
namespace
{
	constexpr u32 SPU_STOP = 0x00000000u; // stop 0x0 — the no-return trap word

	// RI16: il rt, i16  (opcode 0x081)
	constexpr u32 enc_il(u32 rt, u32 imm)
	{
		return (0x081u << 23) | ((imm & 0xffff) << 7) | (rt & 0x7f);
	}

	// RI16: brsl rt, target  (opcode 0x066). Branch-relative-and-set-link (call).
	constexpr u32 enc_brsl(u32 pos, u32 target)
	{
		const u32 rel = ((target - pos) / 4) & 0xffff;
		return (0x066u << 23) | (rel << 7) | 0u /* rt = $lr ($0) */;
	}

	// RR: bi $0  (opcode 0x1a8) — branch indirect to $lr, i.e. function return.
	constexpr u32 enc_bi_lr()
	{
		return 0x1a8u << 21;
	}
}

TEST(SpuAnalyserGiga, ReturnToStopTrapDoesNotRangeCheckFail)
{
	const auto saved = g_cfg.core.spu_block_size.get();
	g_cfg.core.spu_block_size.set(spu_block_size_type::giga);

	auto rec = spu_recompiler_base::make_asmjit_recompiler();
	ASSERT_TRUE(rec);

	std::array<be_t<u32>, SPU_LS_SIZE / 4> ls{};
	const auto w = [&](u32 addr, u32 instr) { ls[addr / 4] = instr; };

	// entry@0x00 calls funcA@0x10; the call's return address (0x04) is a stop
	// trap, sitting immediately before funcB@0x08 which funcA also calls — so the
	// return-point block at 0x04 is created, then removed (stop word), orphaning it.
	w(0x00, enc_brsl(0x00, 0x10)); // entry -> funcA ; return = 0x04
	w(0x04, SPU_STOP);             // <- the trap that drives the bug
	w(0x08, enc_il(3, 7));         // funcB@0x08
	w(0x0c, enc_bi_lr());          // funcB return
	w(0x10, enc_brsl(0x10, 0x08)); // funcA@0x10 -> funcB ; return = 0x14
	w(0x14, enc_bi_lr());          // funcA return

	std::map<u32, std::vector<u32>> targets;

	// Pre-fix this aborts inside analyse() (Range check failed); reaching the
	// assertions is the regression check.
	const spu_program prog = rec->analyse(ls.data(), 0x00, &targets);
	EXPECT_EQ(prog.entry_point, 0x00u);
	EXPECT_FALSE(prog.data.empty());

	g_cfg.core.spu_block_size.set(saved);
}
