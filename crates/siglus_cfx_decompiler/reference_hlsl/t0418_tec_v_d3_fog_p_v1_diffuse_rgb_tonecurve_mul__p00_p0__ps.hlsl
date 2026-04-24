uniform sampler2D s0;
uniform sampler2D s1;

static const float4 c9 = float4(1.0, 0.5, -1.0, 0.0);

struct PS_INPUT {
    float4 v0 : COLOR0;
    float4 t1 : TEXCOORD1;
    float4 t2 : TEXCOORD2;
};

struct PS_OUTPUT {
    float4 oC0 : COLOR0;
};

PS_OUTPUT main(PS_INPUT input) {
    PS_OUTPUT output;
    output.oC0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r0 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r1 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r2 = float4(0.0, 0.0, 0.0, 0.0);

    r0.w = (1.0 / input.t2.w);
    r0.x = (input.t2.x * r0.w + c9.x);
    r0.y = (input.t2.y * r0.w + c9.x);
    r0.x = (r0.x * c3.z);
    r1.w = c9.y;
    r0.z = (r0.x * r1.w + c3.x);
    r0.x = (r0.y * -(c9.y) + c9.x);
    r0.xy = (r0.zx * c3.ww + c3.yy);
    r0 = tex2D(s1, r0.xy);
    r1.xyz = (-(input.t1.xyz) + c2.xyz);
    r1.x = dot(r1.xyz, r1.xyz);
    r1.x = rsqrt(r1.x);
    r1.x = (1.0 / r1.x);
    r1.x = (r1.x + -(c4.y));
    r1.x = saturate((r1.x * c0.x));
    r2 = lerp(input.v0, r0, r1.xxxx);
    r0 = (c1.xxxx >= 0 ? r2 : input.v0);
    r0.w = dot(c5, r0);
    r1.xyz = lerp(r0.xyz, r0.www, c8.yyy);
    r1.w = c8.x;
    r0.x = r1.x;
    r0.y = r1.w;
    r1.x = r1.y;
    r1.y = r1.w;
    r2.x = r1.z;
    r2.y = r1.w;
    r0 = tex2D(s0, r0.xy);
    r1 = tex2D(s0, r1.xy);
    r2 = tex2D(s0, r2.xy);
    r2.x = r0.x;
    r2.y = r1.y;
    r0.xyz = lerp(r2.xyz, c6.xyz, c6.www);
    r0.xyz = (r0.xyz + c7.xyz);
    r0.xyz = (r0.xyz + c9.zzz);
    r0.xyz = (input.v0.www * r0.xyz + c9.xxx);
    r0.w = input.v0.w;
    output.oC0 = r0;
    return output;
}
