package com.localtodo.desktop

import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build

/** 系统闹钟到点后展示提醒通知；不修改任何任务数据 */
class ReminderReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val taskId = intent.getStringExtra(ReminderScheduler.EXTRA_TASK_ID) ?: return
        val title = intent.getStringExtra(ReminderScheduler.EXTRA_TITLE)?.takeIf { it.isNotBlank() }
            ?: "任务提醒"
        val body = intent.getStringExtra(ReminderScheduler.EXTRA_BODY) ?: ""
        val notificationId = intent.getIntExtra(
            ReminderScheduler.EXTRA_NOTIFICATION_ID,
            taskId.hashCode() and 0x7fffffff
        )
        ReminderScheduler.ensureChannel(context)
        // 先移除持久化记录，避免重启后重复触发
        ReminderScheduler.removeStored(context, taskId)

        val launch = context.packageManager.getLaunchIntentForPackage(context.packageName)
        val contentIntent = launch?.let {
            it.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
            PendingIntent.getActivity(
                context,
                notificationId,
                it,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
            )
        }

        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(context, ReminderScheduler.CHANNEL_ID)
        } else {
            Notification.Builder(context).setPriority(Notification.PRIORITY_HIGH)
        }
        builder
            .setSmallIcon(context.applicationInfo.icon)
            .setContentTitle(title)
            .setContentText(body)
            .setStyle(Notification.BigTextStyle().bigText(body))
            .setAutoCancel(true)
            .setWhen(System.currentTimeMillis())
            .setShowWhen(true)
        if (contentIntent != null) builder.setContentIntent(contentIntent)

        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        runCatching { manager.notify(notificationId, builder.build()) }
    }
}
