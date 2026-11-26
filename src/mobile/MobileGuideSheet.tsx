import { IconX } from "../components/Icons";
import { MOBILE_GUIDE } from "./mobileGuide";

interface MobileGuideSheetProps {
  onClose: () => void;
}

/** 手机端使用说明：整页弹出层，内容独立维护，便于后续调整文案 */
export function MobileGuideSheet({ onClose }: MobileGuideSheetProps) {
  return (
    <div className="mobile-sheet" role="dialog" aria-modal="true">
      <div className="mobile-sheet-backdrop" onClick={onClose} />
      <div className="mobile-sheet-panel mobile-guide-panel">
        <header className="mobile-sheet-header">
          <h2>使用说明</h2>
          <button className="mobile-icon-button" onClick={onClose} aria-label="关闭">
            <IconX size={20} />
          </button>
        </header>
        <div className="mobile-sheet-body mobile-guide-body">
          {MOBILE_GUIDE.map((section) => (
            <section className="mobile-guide-section" key={section.title}>
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
              {section.note && <p className="mobile-guide-note">{section.note}</p>}
            </section>
          ))}
        </div>
      </div>
    </div>
  );
}
