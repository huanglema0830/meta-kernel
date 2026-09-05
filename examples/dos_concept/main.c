/*
 * examples/dos_concept/main.c — DOSBox 虚拟喇叭 · 概念演示
 *
 * 当前阶段为"概念演示层"：用 C 语言展示通过 bridge.h 的三个接口
 * 驱动内核、控制虚拟喇叭发声的方式。编译后即可在本机/CI 运行；
 * 后续若做 DOSBox 实机验证，本代码可直接交给 Turbo C / Open Watcom
 * 编译进 DOS 环境（接口与平台无关）。
 *
 * 编译（仓库根目录）：
 *   cc examples/dos_concept/main.c -I npb -o dos_demo \
 *      -L target/debug -lnpb -Wl,-rpath,"$PWD/target/debug"
 *   ./dos_demo
 */
#include <stdio.h>
#include "bridge.h"

/* 虚拟喇叭：内核输出超过阈值则"发声"一次 */
static void virtual_beep(float out, float ent) {
    if (out > 0.55f) {
        /* 音高随熵值滑移：熵高（乱）→ 高频急促，熵低（稳）→ 低频沉稳 */
        float pitch = 440.0f + (1.0f - ent) * 440.0f;
        printf("[SPEAKER] BEEP @ %.1f Hz  (kernel out=%.3f ent=%.3f)\n",
               pitch, out, ent);
    }
}

int main(void) {
    printf("=== meta-kernel NPB · DOSBox virtual speaker (concept demo) ===\n");
    printf("0-anchor boot: push first perturbation 1.0 ...\n");

    int beeps = 0;
    for (int i = 0; i < 48; i++) {
        /* 呼吸式种子：周期脉冲 + 平稳消耗混合，偶发成对突发制造干涉 */
        float seed;
        if (i % 8 == 0) {
            seed = 1.0f;                  /* 脉冲（第一扰动/心跳） */
        } else if (i % 3 == 0) {
            seed = 0.9f;                  /* 突发扰动 */
        } else {
            seed = 0.25f + 0.05f * (float)(i % 5); /* 平稳流入 */
        }

        push_seed(seed);                  /* ① 注种子 */

        /* 读取 1~2 个输出，驱动喇叭 */
        float out = pop_result();         /* ② 取输出 */
        if (out > 0.0f) {
            virtual_beep(out, get_entropy()); /* ③ 问熵 → 决定音高/节律 */
            beeps++;
        }
    }

    printf("--- demo complete: %d beep(s) produced ---\n", beeps);
    printf("KERNEL_DEMO_DONE\n");
    return 0;
}
