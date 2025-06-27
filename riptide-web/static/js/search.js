import{A as u}from"./api-client-touATVF3.js";import{a as h}from"./formatting-Df0aMrHg.js";class y{constructor(){this.apiClient=new u}initialize(){this.setupEventListeners(),this.autoFocusSearchInput()}setupEventListeners(){const s=document.querySelector(".search-input-form");s&&s.addEventListener("submit",e=>this.performSearch(e))}autoFocusSearchInput(){const s=document.getElementById("search-query");s&&s.focus()}async performSearch(s){s.preventDefault();const e=document.getElementById("search-query"),t=document.querySelector('input[name="category"]:checked');if(!e||!t)return;const n=e.value.trim(),l=t.value;if(!n)return;const o=document.getElementById("search-loading"),r=document.getElementById("search-results"),i=document.getElementById("results-grid");if(!(!o||!r||!i)){o.style.display="block",r.style.display="none";try{const a=await this.apiClient.searchMedia(n,l);o.style.display="none",a&&a.length>0?(this.renderSearchResults(a),r.style.display="block"):(i.innerHTML='<p class="no-results">No results found.</p>',r.style.display="block")}catch(a){o.style.display="none";const c=a instanceof Error?a.message:"Unknown error";i.innerHTML=`<p class="error">Search failed: ${this.escapeHtml(c)}</p>`,r.style.display="block"}}}renderSearchResults(s){const e=document.getElementById("results-grid");e&&(e.innerHTML="",s.forEach(t=>{const n=document.createElement("div");n.className="search-result-card";const l=t.poster_url?`<img src="${t.poster_url}" alt="${t.title}" class="result-poster" loading="lazy">`:'<div class="result-poster-placeholder"></div>',o=t.year?` (${t.year})`:"",r=t.rating?`<span class="rating">★ ${t.rating}</span>`:"",i=t.genre?`<span class="genre">${this.escapeHtml(t.genre)}</span>`:"";n.innerHTML=`
                <div class="result-poster-container">
                    ${l}
                </div>
                <div class="result-info">
                    <h3 class="result-title">${this.escapeHtml(t.title)}${o}</h3>
                    <div class="result-meta">
                        ${r}
                        ${i}
                        <span class="media-type">${this.escapeHtml(t.media_type)}</span>
                    </div>
                    <p class="result-plot">${this.escapeHtml(t.plot||"No description available.")}</p>
                    <div class="result-torrents">
                        <h4>Available Downloads (${t.torrents.length})</h4>
                        <div class="torrent-list">
                            ${this.renderTorrentItems(t.torrents.slice(0,3))}
                            ${t.torrents.length>3?`<p class="more-torrents">... and ${t.torrents.length-3} more</p>`:""}
                        </div>
                    </div>
                </div>
            `,n.querySelectorAll("[data-magnet-link]").forEach(c=>{c.addEventListener("click",m=>{const d=m.target.getAttribute("data-magnet-link");d&&this.downloadTorrent(d)})}),e.appendChild(n)}))}renderTorrentItems(s){return s.map(e=>`
            <div class="torrent-item">
                <div class="torrent-info">
                    <span class="torrent-quality">${this.escapeHtml(e.quality)}</span>
                    <span class="torrent-size">${h(e.size)}</span>
                    <span class="torrent-seeds">🌱 ${e.seeders}</span>
                </div>
                <button class="btn btn-sm btn-primary" data-magnet-link="${this.escapeHtml(e.magnet_link)}">
                    Download
                </button>
            </div>
        `).join("")}async downloadTorrent(s){try{const e=await this.apiClient.addTorrent(s);e.success?(alert("Torrent added successfully! "+e.message),e.stream_url&&confirm("Would you like to start streaming now?")&&window.open(e.stream_url,"_blank")):alert("Failed to add torrent: "+e.message)}catch(e){const t=e instanceof Error?e.message:"Unknown error";alert("Error adding torrent: "+t)}}escapeHtml(s){const e=document.createElement("div");return e.textContent=s,e.innerHTML}}document.addEventListener("DOMContentLoaded",()=>{new y().initialize()});
