/*
 * npb/bridge.h — NPB（Nothing-to-Physics Bridge）C 头文件
 *
 * 把 meta-kernel-core 暴露给任何 C 宿主（DOS、嵌入式、桌面程序…）。
 * 宿主只需三个动作：push_seed（注入扰动）、pop_result（取输出）、
 * get_entropy（问系统熵/健康度）。
 *
 * 链接：原生 cdylib（libnpb.so/.dll/.dylib）或 WASM 导出同名符号。
 */
#ifndef META_KERNEL_NPB_BRIDGE_H
#define META_KERNEL_NPB_BRIDGE_H

#ifdef __cplusplus
extern "C" {
#endif

/* 注入扰动（0~1；负值归零，超限钳位）。 */
void push_seed(float value);

/* 读取内核输出（FIFO；无输出时返回 0.0）。 */
float pop_result(void);

/* 获取系统熵值/健康度（0~1；无样本时返回 1.0 = 真空/完全不确定）。 */
float get_entropy(void);

#ifdef __cplusplus
}
#endif

#endif /* META_KERNEL_NPB_BRIDGE_H */
