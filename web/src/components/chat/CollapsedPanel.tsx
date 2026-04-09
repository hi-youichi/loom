import { memo } from "react"
import { MessageSquare } from "lucide-react"
import { cn } from "@/lib/utils"

interface CollapsedPanelProps {
  unreadCount: number
  onExpand: () => void
}

export const CollapsedPanel = memo(function CollapsedPanel({
  unreadCount,
  onExpand,
}: CollapsedPanelProps) {
  return (
    <button
      onClick={onExpand}
      className={cn(
        "h-full w-12 flex flex-col items-center justify-center gap-2",
        "bg-background border-l border-border",
        "hover:bg-accent/50 transition-colors cursor-pointer"
      )}
      aria-label={`查看 ${unreadCount} 条未读消息`}
    >
      <MessageSquare className="h-5 w-5 text-muted-foreground" aria-hidden />
      {unreadCount > 0 && (
        <span className="text-xs font-medium text-primary">{unreadCount}</span>
      )}
    </button>
  )
})
