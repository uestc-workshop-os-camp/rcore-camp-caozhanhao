**实现功能**
----------------
- 记录第一次被调度时刻的时间。
- 记录syscall次数和每次调用时间， 并计算距离任务第一次被调度时刻的时长。


**简答**
----------------
1. 报错如下
```
[kernel] PageFault in application, bad addr = 0x0, bad instruction = 0x804003ac, kernel killed it.
[kernel] IllegalInstruction in application, kernel killed it.
[kernel] IllegalInstruction in application, kernel killed it.
```
分别为
- StoreFault
- IllegalInstruction
- IllegalInstruction  
版本: `RustSBI version 0.3.0-alpha.2, adapting to RISC-V SBI v1.0.0`

2. 如下
- `a0`指向上一个TaskContext, 与`__restore`无关。`__restore`可以恢复当前程序陷入Trap或运行结束需进行下一个程序初始化的Trap上下文；在本章中，也可为任务切换时需恢复的Trap上下文
- `sstatus`中SPP 等字段给出 Trap 发生之前 CPU 处在哪个特权级（S/U）等信息，`sepc`记录 Trap 发生之前执行的最后一条指令的地址, `scause`描述 Trap 的原因。`__restore`需恢复这些寄存器以便`sret`返回到正确的特权级以及指令地址。这里的特殊处理是为了防止嵌套的`Trap`覆盖掉当前信息。
- `x2` 为 `sp`, 会在`trap_handler`调用后恢复， [关于x4](https://riscv-rtthread-programming-manual.readthedocs.io/zh-cn/latest/zh_CN/3.html),它指向线程特定变量，暂时用不到。
- `sp`恢复为用户栈, `sscratch`指向内核栈。
- `sret`
- 交换`sp`和`sscratch`, 此后`sp`指向内核栈, `sscratch`指向用户栈。
- `ecall`


**荣誉准则**
----------------

1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 **以下各位** 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

    暂无交流

2. 此外，我也参考了 **以下资料** ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

    [rCore-Tutorial-Guide 2024 春季学期](https://learningos.cn/rCore-Tutorial-Guide-2024S)  
    [rCore-Tutorial-Book-v3 3.6.0-alpha.1 文档](https://rcore-os.cn/rCore-Tutorial-Book-v3)
    [RISC-V手册](http://riscvbook.com/chinese/RISC-V-Reader-Chinese-v2p1.pdf)

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。
我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。
我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。
我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。
我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。
