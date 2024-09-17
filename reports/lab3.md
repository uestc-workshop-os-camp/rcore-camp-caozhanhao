**实现功能**
----------------
- 实现了新的系统调用`sys_spawn`
- 实现了stride调度算法

**简答**
----------------   
1. 实际情况是轮到 p1 执行吗？为什么？    
不是，因为p2执行后其stride为260->4，其步长更小。
2. 证明 *STRIDE_MAX – STRIDE_MIN <= BigStride / 2*    
由 *prioriry >= 2* 得，*PASS_MAX = BigStride / prioriry <= BigStride / 2*    
而当每个进程 *stride* 相同时，无论经过几次调度，总有 *STRIDE_MAX – STRIDE_MIN <= PASS_MAX*（若不成立则上一次调度错误)  
则有 *STRIDE_MAX – STRIDE_MIN <= BigStride / 2*
3. 如下
```
use core::cmp::{Ordering, max, min};
const BIG_STRIDE : u64 = 255;
struct Stride(u64);

impl PartialOrd for Stride {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let diff = max(self.0, other.0) - min(self.0, other.0);
        if diff > BIG_STRIDE / 2 {
            other.0.partial_cmp(&self.0)
        }
        else {
            self.0.partial_cmp(&other.0)
        }
    }
}

impl PartialEq for Stride {
    fn eq(&self, other: &Self) -> bool {
        false
    }
}
```

**关于stride算法**
----------------
在[Carl A. Waldspurger的论文](https://waldspurger.org/carl/papers/phd-mit-tr667.pdf)中指出，实际应用中，当我们使用64位整数时，溢出的问题可以忽略。举个例子，当*BigStride = 2^20*，*prioriry = 1*时，要走约2^44步才会溢出。即使每次1ms也要几百世纪才会溢出。


**荣誉准则**
----------------

1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 **以下各位** 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

    暂无交流

2. 此外，我也参考了 **以下资料** ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

    [rCore-Tutorial-Guide 2024 春季学期](https://learningos.cn/rCore-Tutorial-Guide-2024S)  
    [rCore-Tutorial-Book-v3 3.6.0-alpha.1 文档](https://rcore-os.cn/rCore-Tutorial-Book-v3)  
    [Lottery and Stride Scheduling: Flexible Proportional-Share Resource Management by Carl A. Waldspurger](https://waldspurger.org/carl/papers/phd-mit-tr667.pdf)  

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。
我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。
我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。
我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。
我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。
