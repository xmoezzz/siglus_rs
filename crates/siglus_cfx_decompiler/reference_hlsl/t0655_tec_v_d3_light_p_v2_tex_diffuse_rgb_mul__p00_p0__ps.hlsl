uniform sampler2D s0;
uniform sampler2D s1;

static const float4 c10 = float4(0.0005, 1.0, 0.5, -1.0);

struct PS_INPUT {
    float4 v0 : COLOR0;
    float4 t0 : TEXCOORD0;
    float4 t1 : TEXCOORD1;
    float4 t2 : TEXCOORD2;
    float4 t3 : TEXCOORD3;
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

    r0.w = (1.0 / input.t3.w);
    r0.x = (input.t3.x * r0.w + c10.y);
    r0.y = (input.t3.y * r0.w + c10.y);
    r0.x = (r0.x * c6.z);
    r0.z = c10.z;
    r0.z = (r0.x * r0.z + c6.x);
    r0.x = (r0.y * -(c10.z) + c10.y);
    r0.xy = (r0.zx * c6.ww + c6.yy);
    r0 = tex2D(s1, r0.xy);
    r1 = tex2D(s0, input.t0.xy);
    r2.xyz = (-(input.t2.xyz) + c3.xyz);
    r0.w = dot(r2.xyz, r2.xyz);
    r0.w = rsqrt(r0.w);
    r0.w = (1.0 / r0.w);
    r0.w = (r0.w + -(c7.y));
    r0.w = saturate((r0.w * c1.x));
    r2.xyz = (-(input.t2.xyz) + c4.xyz);
    r2.w = dot(r2.xyz, r2.xyz);
    r2.w = rsqrt(r2.w);
    r2.xyz = (r2.xyz * r2.www);
    r2.w = (1.0 / r2.w);
    r3.xyz = normalize(input.t1.xyz);
    r2.x = dot(r3.xyz, r2.xyz);
    r2.y = (r2.w * -(c10.x) + c10.y);
    r2.x = saturate((r2.x * r2.y));
    r1 = (r1 * input.v0);
    r2.xyz = (r2.xxx * r1.xyz);
    r2.xyz = (r2.xyz * c5.xyz);
    r2.xyz = (c0.xxx >= 0 ? r2.xyz : r1.xyz);
    r3.xyz = lerp(r2.xyz, r0.xyz, r0.www);
    r0.xyz = (c2.xxx >= 0 ? r3.xyz : r2.xyz);
    r2.xyz = lerp(r0.xyz, c8.xyz, c8.www);
    r0.xyz = (r2.xyz + c9.xyz);
    r0.xyz = (r0.xyz + c10.www);
    r1.xyz = (r1.www * r0.xyz + c10.yyy);
    output.oC0 = r1;
    return output;
}
