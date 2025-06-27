import{A as o}from"./api-client-touATVF3.js";import{a as l}from"./formatting-Df0aMrHg.js";class c{constructor(){this.libraryItems=[],this.apiClient=new o}async initialize(t){this.libraryItems=t,this.setupEventListeners(),this.renderLibraryItems(this.libraryItems)}setupEventListeners(){const t=document.getElementById("search"),i=document.getElementById("type-filter");t&&t.addEventListener("input",e=>{const a=e.target.value.toLowerCase(),s=this.libraryItems.filter(r=>r.title.toLowerCase().includes(a));this.renderLibraryItems(s)}),i&&i.addEventListener("change",e=>{const a=e.target.value,s=a?this.libraryItems.filter(r=>r.media_type===a):this.libraryItems;this.renderLibraryItems(s)})}renderLibraryItems(t){const i=document.getElementById("media-grid");i&&(i.innerHTML="",t.forEach(e=>{const a=document.createElement("div");a.className="media-card";const s=e.thumbnail_url?`<img src="${e.thumbnail_url}" alt="${e.title}" loading="lazy">`:'<div class="placeholder-poster"></div>',r=e.duration?`<p class="media-duration">${Math.floor(e.duration/60)}m</p>`:"";a.innerHTML=`
                <div class="media-poster">
                    ${s}
                </div>
                <div class="media-info">
                    <h3 class="media-title">${this.escapeHtml(e.title)}</h3>
                    <p class="media-type">${this.escapeHtml(e.media_type)}</p>
                    <p class="media-size">${l(e.size)}</p>
                    ${r}
                    <div class="media-actions">
                        <a href="${e.stream_url}" class="btn btn-primary">Stream</a>
                        <button class="btn btn-secondary" data-media-id="${e.id}">Details</button>
                    </div>
                </div>
            `;const d=a.querySelector("[data-media-id]");d&&d.addEventListener("click",()=>{this.showMediaDetails(e.id)}),i.appendChild(a)}))}async showMediaDetails(t){try{const i=await this.apiClient.getMediaDetails(t);this.createDetailsModal(i)}catch(i){console.error("Failed to load media details:",i)}}createDetailsModal(t){const i=document.getElementById("media-details-modal");i&&i.remove();const e=document.createElement("div");e.id="media-details-modal",e.className="modal",e.innerHTML=`
            <div class="modal-content">
                <div class="modal-header">
                    <h2>${this.escapeHtml(t.title)}</h2>
                    <button class="modal-close">&times;</button>
                </div>
                <div class="modal-body">
                    <div class="media-details">
                        <div class="media-poster">
                            ${t.thumbnail_url?`<img src="${t.thumbnail_url}" alt="${t.title}">`:'<div class="placeholder-poster"></div>'}
                        </div>
                        <div class="media-info">
                            <p><strong>Type:</strong> ${this.escapeHtml(t.media_type)}</p>
                            <p><strong>Size:</strong> ${l(t.size)}</p>
                            ${t.duration?`<p><strong>Duration:</strong> ${Math.floor(t.duration/60)}m</p>`:""}
                            <div class="media-actions">
                                <a href="${t.stream_url}" class="btn btn-primary">Stream Now</a>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        `,e.querySelector(".modal-close").addEventListener("click",()=>e.remove()),e.addEventListener("click",s=>{s.target===e&&e.remove()}),document.body.appendChild(e)}escapeHtml(t){const i=document.createElement("div");return i.textContent=t,i.innerHTML}}document.addEventListener("DOMContentLoaded",()=>{const n=window.libraryItems||[];new c().initialize(n)});
