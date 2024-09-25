**实现功能**
----------------
- 实现了死锁检测

**简答**
----------------
1. 在我们的多线程实现中，当主线程 (即 0 号线程) 退出时，视为整个进程退出， 此时需要结束该进程管理的所有线程并回收其资源。  
(1) 需要回收的资源有哪些？  
TaskControlBlock(内核栈、用户栈、task id、task上下文、trap上下文)  
(2) 其他线程的 TaskControlBlock 可能在哪些位置被引用，分别是否需要回收，为什么？  
   - exit_current_and_run_next()，会回收  
   - sys_waittid(), 会回收  
   - 其他位置出现的为借用，不会回收资源
   - 任务已结束所以要及时回收资源  

2. 对比以下两种 Mutex.unlock 的实现，二者有什么区别？这些区别可能会导致什么问题？  
```rust
 impl Mutex for Mutex1 {
     fn unlock(&self) {
         let mut mutex_inner = self.inner.exclusive_access();
         assert!(mutex_inner.locked);
         mutex_inner.locked = false;
         if let Some(waking_task) = mutex_inner.wait_queue.pop_front() {
             add_task(waking_task);
         }
     }
}

impl Mutex for Mutex2 {
    fn unlock(&self) {
        let mut mutex_inner = self.inner.exclusive_access();
        assert!(mutex_inner.locked);
        if let Some(waking_task) = mutex_inner.wait_queue.pop_front() {
            add_task(waking_task);
        } else {
            mutex_inner.locked = false;
        }
    }
}
```
- 第一种当一个线程即将被唤醒，一个线程即将要拿到锁的时候存在竞态条件。简单来说，当被唤醒的线程还没来得及将`locked`置为true时，另一个线程发现`locked`为false，也获得了锁。也就是说，此时两个线程同时获得了锁。
- 第二种即为rcore中的做法，在有线程等待时直接将锁交接给其他线程。注意到这个操作对其他线程是透明的，在它们看来锁从没有开过。这使后来的线程总是在队列的最后，因此更加公平。
```rust
impl Mutex for Mutex2 {
    fn lock(&self) {
        let mut mutex_inner = self.inner.exclusive_access();
        if mutex_inner.locked {
            mutex_inner.wait_queue.push_back(current_task().unwrap());
            drop(mutex_inner);
            block_current_and_run_next();
        } else {
            mutex_inner.locked = true;
        }
    }
}
```

**荣誉准则**
----------------

1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 **以下各位** 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

    暂无交流

2. 此外，我也参考了 **以下资料** ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

    [rCore-Tutorial-Guide 2024 春季学期](https://learningos.cn/rCore-Tutorial-Guide-2024S)  
    [rCore-Tutorial-Book-v3 3.6.0-alpha.1 文档](https://rcore-os.cn/rCore-Tutorial-Book-v3)

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。
我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。
我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。
我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。
我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。
