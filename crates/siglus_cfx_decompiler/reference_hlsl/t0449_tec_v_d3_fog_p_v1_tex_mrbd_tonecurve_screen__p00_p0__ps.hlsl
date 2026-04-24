uniform sampler2D s0;
uniform sampler2D s1;
uniform sampler2D s2;

static const float4 c8 = float4(1.0, 0.5, -2.0, 0.0);

struct PS_INPUT {
    float4 v0 : COLOR0;
    float4 t0 : TEXCOORD0;
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
    float4 r3 = float4(0.0, 0.0, 0.0, 0.0);
    float4 r4 = float4(0.0, 0.0, 0.0, 0.0);

    r0.xyz = (-(input.t1.xyz) + c2.xyz);
    r0.x = dot(r0.xyz, r0.xyz);
    r0.x = rsqrt(r0.x);
    r0.x = (1.0 / r0.x);
    r0.x = (r0.x + -(c4.y));
    r0.x = saturate((r0.x * c0.x));
    r0.y = (1.0 / input.t2.w);
    r0.z = (input.t2.x * r0.y + c8.x);
    r0.y = (input.t2.y * r0.y + c8.x);
    r0.z = (r0.z * c3.z);
    r0.w = c8.y;
    r0.w = (r0.z * r0.w + c3.x);
    r0.z = (r0.y * -(c8.y) + c8.x);
    r1.xy = (r0.wz * c3.ww + c3.yy);
    r1 = tex2D(s2, r1.xy);
    r2 = tex2D(s0, input.t0.xy);
    r1 = (input.v0 * -(r2) + r1);
    r2 = (r2 * input.v0);
    r0 = (r0.xxxx * r1 + r2);
    r0 = (c1.xxxx >= 0 ? r0 : r2);
    r0.w = dot(c5, r0);
    r1.xyz = lerp(r0.xyz, r0.www, c7.yyy);
    r1.w = c7.x;
    r0.x = r1.x;
    r0.y = r1.w;
    r1.x = r1.y;
    r1.y = r1.w;
    r3.x = r1.z;
    r3.y = r1.w;
    r4 = tex2D(s1, r0.xy);
    r1 = tex2D(s1, r1.xy);
    r3 = tex2D(s1, r3.xy);
    r3.x = r4.x;
    r3.y = r1.y;
    r0.xyz = (r3.xyz * c8.zzz + c8.xxx);
    r0.xyz = (c6.yyy * r0.xyz + r3.xyz);
    r1.xyz = lerp(r0.xyz, r0.www, c6.xxx);
    r0.xyz = (r1.xyz + c6.zzz);
    r0.xyz = (r0.xyz + -(c6.www));
    r2.xyz = (r2.www * r0.xyz);
    output.oC0 = r2;
    return output;
}
