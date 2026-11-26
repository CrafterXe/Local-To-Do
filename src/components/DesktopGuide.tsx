import { useState } from "react";

interface GuideSection {
  title: string;
  paragraphs?: string[];
  items?: string[];
  note?: string;
}

/** Windows 端使用说明：仅描述当前版本真实具备的功能 */
const DESKTOP_GUIDE: GuideSection[] = [
  {
    title: "基本介绍",
    paragraphs: [
      "Local To Do 是一款本地优先的任务管理工具，Windows 端是唯一的数据管理端，所有任务、清单、提醒和备份都保存在本机。",
      "数据保存在本机 SQLite 数据库中，不联网、无账号、无遥测；Android 手机端只通过家庭局域网读取数据。",
    ],
  },
  {
    title: "核心功能",
    items: [
      "任务：新建、编辑、删除、完成与取消完成，可标记为重要。",
      "清单：按清单归类任务，支持排序、改名与删除。",
      "我的今天：汇总今天到期、今天完成以及逾期任务。",
      "我的日历：按月查看每天的任务安排。",
      "到期时间与提醒：设置到期日期、时间与提前提醒。",
      "重复任务：按每日、每周、每月等规则重复。",
      "子步骤与备注：为任务拆分步骤、记录补充说明。",
      "最近删除：删除的任务与清单可在回收站中恢复或彻底清除。",
      "备份与恢复：手动备份、自动备份，支持导出 JSON / CSV 与从备份恢复。",
      "局域网同步：开启同步服务，与 Android 手机配对后单向推送数据。",
    ],
  },
  {
    title: "常用操作",
    items: [
      "新建任务：在任务列表顶部输入标题后回车，再点开任务补充详细信息。",
      "设置到期时间：在任务详情中选择到期日期与具体时间。",
      "设置提醒：在任务详情中开启提醒并选择提前量，电脑会在设定时间弹出提醒。",
      "创建清单：在左侧清单区域新建清单，并把任务移动到对应清单。",
      "重复任务：在任务详情中启用重复规则并选择重复方式。",
      "查看日历：打开「我的日历」按日期查看安排，点击日期查看当天任务。",
      "管理已完成：在任务列表中切换显示已完成任务，可取消完成恢复为待办。",
      "连接手机：打开「设置 → 局域网同步」，开启服务并用手机扫描二维码完成配对。",
    ],
  },
  {
    title: "同步说明",
    items: [
      "Windows 是权威数据源，所有修改都只在本机进行。",
      "Android 通过家庭局域网获取任务数据，只读查看，不能修改任务。",
      "手机端修改任务的功能不受支持；如需调整任务，请在 Windows 端操作后让手机同步。",
      "局域网同步需要 Windows 端开启同步服务，并完成一次设备配对。",
    ],
    note: "同步服务仅监听本机的私有局域网地址，不会暴露到公网。",
  },
  {
    title: "数据与隐私",
    paragraphs: [
      "所有数据保存在本机，不上传任何云端；备份文件与日志目录可在「设置 → 数据与备份」中查看。",
    ],
  },
];

/** 设置页中的「说明」栏目：默认折叠，展开后按分组展示 */
export function DesktopGuide() {
  const [open, setOpen] = useState(false);
  return (
    <section className="settings-section">
      <h2 className="settings-title">说明</h2>
      <div className="setting-row">
        <div className="setting-text">
          <div className="setting-name">使用说明</div>
          <div className="setting-hint">
            介绍主要功能、常用操作与手机同步方式。
          </div>
        </div>
        <button className="btn small" onClick={() => setOpen((v) => !v)}>
          {open ? "收起" : "查看说明"}
        </button>
      </div>
      {open && (
        <div className="guide-panel">
          {DESKTOP_GUIDE.map((section) => (
            <div className="guide-section" key={section.title}>
              <h3>{section.title}</h3>
              {section.paragraphs?.map((text) => (
                <p key={text}>{text}</p>
              ))}
              {section.items && (
                <ul>
                  {section.items.map((item) => (
                    <li key={item}>{item}</li>
                  ))}
                </ul>
              )}
              {section.note && <p className="guide-note">{section.note}</p>}
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
