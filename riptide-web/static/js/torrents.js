import{A as d}from"./api-client-touATVF3.js";import{b as c,c as i,a as l}from"./formatting-Df0aMrHg.js";class u{constructor(){this.refreshInterval=null,this.apiClient=new d}async initialize(){this.setupEventListeners(),await this.refreshTorrents(),this.startAutoRefresh()}setupEventListeners(){const t=document.getElementById("refresh-torrents");t&&t.addEventListener("click",()=>this.refreshTorrents())}async refreshTorrents(){try{const t=await this.apiClient.getTorrents();this.renderTorrents(t)}catch(t){console.error("Failed to refresh torrents:",t),this.showError("Failed to load torrents")}}renderTorrents(t){const e=document.getElementById("torrents-tbody");if(e){if(e.innerHTML="",t.length===0){e.innerHTML=`
                <tr>
                    <td colspan="9" class="no-torrents">
                        No torrents found. <a href="/add-torrent">Add your first torrent</a>
                    </td>
                </tr>
            `;return}t.forEach(s=>{const n=document.createElement("tr");n.className=`torrent-row torrent-${s.status}`;const a=this.getStatusClass(s.status),o=this.createProgressBar(s.progress);n.innerHTML=`
                <td class="torrent-name" title="${this.escapeHtml(s.name)}">
                    ${this.escapeHtml(this.truncateText(s.name,40))}
                </td>
                <td class="torrent-status">
                    <span class="status-badge ${a}">${this.capitalizeFirst(s.status)}</span>
                </td>
                <td class="torrent-progress">
                    ${o}
                    <span class="progress-text">${c(s.progress)}</span>
                </td>
                <td class="torrent-download-speed">${i(s.download_speed)}</td>
                <td class="torrent-upload-speed">${i(s.upload_speed)}</td>
                <td class="torrent-size">${l(s.size)}</td>
                <td class="torrent-ratio">${s.ratio.toFixed(2)}</td>
                <td class="torrent-peers">${s.peer_count}</td>
                <td class="torrent-actions">
                    ${this.createActionButtons(s)}
                </td>
            `,this.addActionListeners(n,s),e.appendChild(n)})}}getStatusClass(t){return{downloading:"status-downloading",seeding:"status-seeding",paused:"status-paused",error:"status-error"}[t]||"status-unknown"}createProgressBar(t){return`
            <div class="progress-bar">
                <div class="progress-fill" style="width: ${Math.round(t*100)}%"></div>
            </div>
        `}createActionButtons(t){const e=[];return t.status==="paused"?e.push('<button class="btn btn-sm btn-success" data-action="resume">Resume</button>'):(t.status==="downloading"||t.status==="seeding")&&e.push('<button class="btn btn-sm btn-warning" data-action="pause">Pause</button>'),t.progress>=1&&e.push('<button class="btn btn-sm btn-primary" data-action="stream">Stream</button>'),e.push('<button class="btn btn-sm btn-danger" data-action="delete">Delete</button>'),e.join(" ")}addActionListeners(t,e){t.querySelectorAll("[data-action]").forEach(n=>{n.addEventListener("click",async a=>{const o=a.target.getAttribute("data-action");await this.handleTorrentAction(e,o)})})}async handleTorrentAction(t,e){if(e)try{switch(e){case"pause":await this.apiClient.pauseTorrent(t.id),await this.refreshTorrents();break;case"resume":await this.apiClient.resumeTorrent(t.id),await this.refreshTorrents();break;case"delete":confirm(`Delete torrent "${t.name}"?`)&&(await this.apiClient.deleteTorrent(t.id),await this.refreshTorrents());break;case"stream":const s=`/stream/${t.id}`;window.open(s,"_blank");break;default:console.warn("Unknown action:",e)}}catch(s){const n=s instanceof Error?s.message:"Unknown error";alert(`Failed to ${e} torrent: ${n}`)}}startAutoRefresh(){this.refreshInterval=window.setInterval(()=>{this.refreshTorrents()},2e3)}stopAutoRefresh(){this.refreshInterval&&(clearInterval(this.refreshInterval),this.refreshInterval=null)}showError(t){const e=document.getElementById("torrents-tbody");e&&(e.innerHTML=`
                <tr>
                    <td colspan="9" class="error-message">
                        ${this.escapeHtml(t)}
                        <button class="btn btn-sm btn-primary" onclick="location.reload()">Retry</button>
                    </td>
                </tr>
            `)}truncateText(t,e){return t.length<=e?t:t.substring(0,e-3)+"..."}capitalizeFirst(t){return t.charAt(0).toUpperCase()+t.slice(1)}escapeHtml(t){const e=document.createElement("div");return e.textContent=t,e.innerHTML}}window.refreshTorrents=()=>{const r=window.torrentManager;r&&r.refreshTorrents()};document.addEventListener("DOMContentLoaded",async()=>{const r=new u;window.torrentManager=r,await r.initialize()});window.addEventListener("beforeunload",()=>{const r=window.torrentManager;r&&r.stopAutoRefresh()});
