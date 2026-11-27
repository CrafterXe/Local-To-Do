package com.localtodo.mobile

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/** 到点后先保存 fired，再展示通知；任务数据由 Rust 在下次重同步时确认。 */
class ReminderReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val taskId = intent.getStringExtra("taskId") ?: return
        val triggerAt = intent.getLongExtra("triggerAt", 0L)
        val requestCode = intent.getIntExtra("requestCode", Int.MIN_VALUE)
        if (triggerAt <= 0L || requestCode == Int.MIN_VALUE) return
        runCatching {
            ReminderScheduler.dispatch(context.applicationContext, taskId, triggerAt, requestCode)
        }
    }
}
