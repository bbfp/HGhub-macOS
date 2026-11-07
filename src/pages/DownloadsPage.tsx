import Downloads from "../components/Downloads";

export default function DownloadsPage({ wide: _wide }: { wide: boolean }) {
  return (
    <div className="absolute inset-0 bg-bg flex flex-col">
      <div className="shrink-0 px-4 pb-2" style={{ paddingTop: "calc(var(--sat, 0px) + 14px)" }}>
        <div className="text-[24px] font-bold text-on leading-none">下载</div>
        <div className="text-[11px] text-mut mt-1.5">按短剧查看下载任务</div>
      </div>
      <div className="flex-1 scroll-y px-4 pb-24">
        <Downloads />
      </div>
    </div>
  );
}
